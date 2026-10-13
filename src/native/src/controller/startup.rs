use std::ffi::c_void;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use super::ffi;
use crate::launch::LaunchOptions;
use crate::session::Session;
use crate::yt_dlp::YtDlp;

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
    launch: LaunchOptions,
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
    let tools = YtDlp::new(&task.library, &task.profile);
    let initial_path = task
        .launch
        .ytdl_path
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            tools
                .as_ref()
                .ok()
                .and_then(|tools| tools.select().ok().flatten())
                .map(|executable| executable.path)
        })
        .unwrap_or_else(|| task.library.clone());
    let result = Session::create(
        &task.library,
        &task.profile,
        task.audio_delay,
        &initial_path,
        &task.launch,
        task.wakeup,
    );
    let started = result.is_ok();
    if task.cancel.load(Ordering::Acquire) {
        return;
    }

    let _ = thread.queue(move |mut player| {
        if player.generation != generation {
            return;
        }
        match result {
            Ok(session) => {
                let external_ytdl = player.launch.ytdl_path.is_some();
                let mut state = player.as_mut().rust_mut();
                state.session = Some(session);
                state.engine_ready = true;
                state.online_ready = external_ytdl;
                state.state = ffi::State::Stopped;
                if (player.applied_audio_delay() - player.audio_delay).abs() > 0.000_001 {
                    let delay = player.audio_delay;
                    player
                        .as_mut()
                        .dispatch(&["set", "audio-delay", &delay.to_string()]);
                }
                // Load a startup request before IPC events record externally loaded media.
                player.as_mut().load_pending_media();
                player.as_mut().drain_events();
                player.as_mut().state_changed();
            }
            Err(error) => {
                player.as_mut().rust_mut().startup_cancel = None;
                player.as_mut().fail(error);
            }
        }
    });
    if !started {
        return;
    }
    if task.launch.ytdl_path.is_some() {
        return;
    }

    let tools = match tools {
        Ok(tools) => tools,
        Err(error) => {
            let _ = thread.queue(move |mut player| {
                if player.generation == generation {
                    player.as_mut().rust_mut().startup_cancel = None;
                    player.as_mut().rust_mut().online_error = Some(error.clone());
                    if player.online_waiting() {
                        player.as_mut().fail(error);
                    }
                }
            });
            return;
        }
    };
    let mut last_percent = -1;
    let installed = tools.ensure_youtube(&task.cancel, |done, total| {
        let percent = (done * 100 / total) as i32;
        if percent == last_percent {
            return;
        }
        last_percent = percent;
        let _ = thread.queue(move |mut player| {
            if player.generation == generation && player.online_waiting() {
                player.as_mut().rust_mut().online_progress = percent;
                player.as_mut().state_changed();
            }
        });
    });
    if task.cancel.load(Ordering::Acquire) {
        return;
    }
    let can_update = matches!(&installed, Ok(Some(_)));
    let _ = thread.queue(move |mut player| {
        if player.generation != generation {
            return;
        }
        match installed {
            Ok(Some(executable)) => player.as_mut().set_online_path(&executable.path),
            Ok(None) => {}
            Err(error) => {
                player.as_mut().rust_mut().online_error = Some(error.clone());
                if player.online_waiting() {
                    player.as_mut().fail(error);
                }
            }
        }
    });
    if can_update {
        let _ = tools.update_if_due(&task.cancel, |_, _| {});
    }
    let _ = thread.queue(move |mut player| {
        if player.generation == generation {
            player.as_mut().rust_mut().startup_cancel = None;
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
    fn set_online_path(mut self: Pin<&mut Self>, path: &std::path::Path) {
        let Some(session) = self.session.clone() else {
            return;
        };
        // change-list append accepts one literal key/value item; unlike the
        // initial script-opts list, commas in the path must not be escaped.
        let option = format!(
            "ytdl_hook-ytdl_path={}",
            path.to_string_lossy().replace('\\', "/")
        );
        match session.command(&["change-list", "script-opts", "append", &option]) {
            Ok(id) => self.as_mut().rust_mut().online_command = Some(id),
            Err(error) => {
                self.as_mut().rust_mut().online_error = Some(error.clone());
                if self.online_waiting() {
                    self.as_mut().fail(error);
                }
            }
        }
    }

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
        state.online_command = None;
        state.online_ready = false;
        state.online_progress = -1;
        state.online_error = None;
        state.engine_ready = false;
        state.file_loaded = false;
        state.pending_commands.clear();
        state.gpu_commands.clear();
        state.gpu.interlaced_seen = false;
        state.gpu.video_signature = None;
        let library_path = PathBuf::from(library.to_string());
        state.gpu.engine_root = library_path.parent().map(PathBuf::from);
        state.gpu.runtime_root = std::env::var_os("QPLAY_MEDIA_RUNTIME").map(PathBuf::from);
        state.state = ffi::State::Starting;
        state.error_text = QString::default();

        let task = StartTask {
            library: library_path,
            profile: PathBuf::from(profile.to_string()),
            audio_delay: state.audio_delay,
            launch: state.launch.clone(),
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

    pub(crate) fn surface_failed(mut self: Pin<&mut Self>, generation: u64, error: &QString) {
        if self.generation == generation {
            self.as_mut().fail(error.to_string());
        }
    }
}
