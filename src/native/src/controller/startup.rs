use std::ffi::c_void;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use super::ffi;
use crate::session::Session;

pub struct Wakeup {
    pub thread: cxx_qt::CxxQtThread<ffi::PlayerController>,
    pub generation: u64,
    scheduled: Arc<AtomicBool>,
}

pub struct FrameWakeup {
    pub thread: cxx_qt::CxxQtThread<ffi::PlayerController>,
    pub generation: u64,
}

pub struct StartTask {
    library: PathBuf,
    profile: PathBuf,
    audio_delay: f64,
    wakeup: Box<Wakeup>,
    cancel: Arc<AtomicBool>,
}

#[expect(
    clippy::boxed_local,
    reason = "CXX transfers this opaque worker task in an owning Rust box"
)]
pub fn run_start_task(task: Box<StartTask>) {
    if task.cancel.load(Ordering::Acquire) {
        return;
    }

    let thread = task.wakeup.thread.clone();
    let generation = task.wakeup.generation;
    let result = Session::create(&task.library, &task.profile, task.audio_delay, task.wakeup);
    if task.cancel.load(Ordering::Acquire) {
        return;
    }

    let _ = thread.queue(move |mut player| {
        if player.generation != generation {
            return;
        }
        player.as_mut().rust_mut().startup_cancel = None;
        match result {
            Ok(session) => {
                let mut state = player.as_mut().rust_mut();
                state.session = Some(session);
                state.engine_ready = true;
                state.state = ffi::State::Stopped;
                if (player.applied_audio_delay() - player.audio_delay).abs() > 0.000_001 {
                    let delay = player.audio_delay;
                    player
                        .as_mut()
                        .dispatch(&["set", "audio-delay", &delay.to_string()]);
                }
                player.as_mut().drain_events();
                player.as_mut().state_changed();
            }
            Err(error) => player.as_mut().fail(error),
        }
    });
}

/// SAFETY: libmpv keeps this userdata valid until native teardown has joined all
/// callbacks. Only the protected CXX-Qt queue is used from the mpv thread.
pub unsafe extern "C" fn wakeup(data: *mut c_void) {
    // SAFETY: Session registered a non-null Box<Wakeup> with the client API.
    let wakeup = unsafe { &*data.cast::<Wakeup>() };
    if wakeup.scheduled.swap(true, Ordering::AcqRel) {
        return;
    }

    let generation = wakeup.generation;
    let scheduled = wakeup.scheduled.clone();
    if wakeup
        .thread
        .queue(move |mut player| {
            scheduled.store(false, Ordering::Release);
            if player.generation == generation {
                player.as_mut().drain_events();
            }
        })
        .is_err()
    {
        wakeup.scheduled.store(false, Ordering::Release);
    }
}

/// SAFETY: The render lease keeps this userdata alive until its render context
/// is freed. No ordinary client API is called from this render callback.
pub unsafe extern "C" fn frame_wakeup(data: *mut c_void) {
    // SAFETY: RenderLease registered a non-null Box<FrameWakeup>.
    let wakeup = unsafe { &*data.cast::<FrameWakeup>() };
    let generation = wakeup.generation;
    let _ = wakeup.thread.queue(move |mut player| {
        if player.generation == generation {
            player.as_mut().frame_ready();
        }
    });
}

impl ffi::PlayerController {
    pub(super) fn initialize(mut self: Pin<&mut Self>, library: &QString, profile: &QString) {
        if let Some(cancel) = &self.startup_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.as_mut().cancel_gpu();

        let cancel = Arc::new(AtomicBool::new(false));
        let thread = self.qt_thread();
        let mut state = self.as_mut().rust_mut();
        state.generation += 1;
        state.session = None;
        state.startup_cancel = Some(cancel.clone());
        state.engine_ready = false;
        state.surface_ready = false;
        state.file_loaded = false;
        state.pending_commands.clear();
        state.gpu_commands.clear();
        state.gpu.interlaced_seen = false;
        let library_path = PathBuf::from(library.to_string());
        state.gpu.engine_root = library_path.parent().map(PathBuf::from);
        state.gpu.player_data = Some(PathBuf::from(profile.to_string()));
        state.state = ffi::State::Starting;
        state.error_text = QString::default();

        let task = StartTask {
            library: library_path,
            profile: PathBuf::from(profile.to_string()),
            audio_delay: state.audio_delay,
            cancel,
            wakeup: Box::new(Wakeup {
                thread,
                generation: state.generation,
                scheduled: Arc::new(AtomicBool::new(false)),
            }),
        };
        ffi::start_session(Box::new(task));
        self.as_mut().state_changed();
    }

    pub(super) fn surface_ready(mut self: Pin<&mut Self>, generation: u64) {
        if self.generation != generation || self.surface_ready {
            return;
        }
        self.as_mut().rust_mut().surface_ready = true;
        self.as_mut().load_pending_media();
    }

    pub(crate) fn surface_failed(mut self: Pin<&mut Self>, generation: u64, error: &QString) {
        if self.generation == generation {
            self.as_mut().fail(error.to_string());
        }
    }
}
