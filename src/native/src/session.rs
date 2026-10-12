use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::controller::{FrameWakeup, Wakeup, ffi};
use crate::mpv::{self, Api, Handle, OpenGlFbo, OpenGlInit, RenderParam};

pub enum PropertyValue {
    Flag(bool),
    Number(f64),
    Text(String),
    Unavailable,
}

pub enum Event {
    Property(String, PropertyValue),
    StartFile,
    FileLoaded,
    EndFile(i32),
    Command(u64, i32),
    Shutdown,
    Other,
}

struct RawSession {
    api: Api,
    handle: Handle,
    _wakeup: Box<Wakeup>,
}

// SAFETY: libmpv's client API is thread-safe. Event polling has one GUI-thread
// owner; render leases call only the render API on their current OpenGL thread.
unsafe impl Send for RawSession {}
unsafe impl Sync for RawSession {}

impl Drop for RawSession {
    fn drop(&mut self) {
        // SAFETY: The handle and callback storage remain valid until all mpv threads
        // are joined. The library outlives native teardown and callback destruction.
        unsafe {
            (self.api.wakeup)(self.handle, None, ptr::null_mut());
            (self.api.terminate)(self.handle);
        }
    }
}

pub struct RetiredSession {
    _raw: RawSession,
}

pub struct Session {
    raw: Option<RawSession>,
    next_command: AtomicU64,
    pub generation: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(raw) = self.raw.take() {
            ffi::retire_session(Box::new(RetiredSession { _raw: raw }));
        }
    }
}

impl Session {
    pub fn create(
        library: &Path,
        profile: &Path,
        audio_delay: f64,
        wakeup: Box<Wakeup>,
    ) -> Result<Arc<Self>, String> {
        let api = Api::load(library)?;
        // SAFETY: The validated library remains owned by RawSession.
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("Could not create the mpv session.".into());
        }
        let raw = RawSession {
            api,
            handle,
            _wakeup: wakeup,
        };
        let watch_later = profile.join("watch-later");
        std::fs::create_dir_all(&watch_later)
            .map_err(|error| format!("Create player profile: {error}"))?;
        let certificate = library
            .parent()
            .ok_or("The mpv library needs an engine directory.")?
            .join("profile/certs/cacert.pem");
        let audio_delay = audio_delay.to_string();
        for (name, value) in [
            ("config", "no"),
            ("vo", "libmpv"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("terminal", "no"),
            ("osc", "no"),
            ("load-scripts", "no"),
            ("load-console", "no"),
            ("load-select", "no"),
            ("ytdl", "no"),
            ("hwdec", "auto"),
            ("tls-verify", "yes"),
            ("resume-playback", "yes"),
            ("save-position-on-quit", "yes"),
            ("audio-delay", audio_delay.as_str()),
            ("sub-pos", "85"),
            ("watch-later-options", "start"),
            ("watch-later-directory", &watch_later.to_string_lossy()),
            ("tls-ca-file", &certificate.to_string_lossy()),
        ] {
            let name = CString::new(name).map_err(|error| error.to_string())?;
            let value = CString::new(value).map_err(|error| error.to_string())?;
            // SAFETY: Strings are terminated and borrowed for this synchronous setup call.
            raw.api
                .check(unsafe { (raw.api.option)(handle, name.as_ptr(), value.as_ptr()) })
                .map_err(|error| format!("Set mpv option {}: {error}", name.to_string_lossy()))?;
        }
        // SAFETY: Initialize is called once, on the startup worker.
        raw.api.check(unsafe { (raw.api.initialize)(handle) })?;
        for (name, format) in [
            ("time-pos", mpv::FORMAT_DOUBLE),
            ("duration", mpv::FORMAT_DOUBLE),
            ("volume", mpv::FORMAT_DOUBLE),
            ("speed", mpv::FORMAT_DOUBLE),
            ("pause", mpv::FORMAT_FLAG),
            ("mute", mpv::FORMAT_FLAG),
            ("track-list", mpv::FORMAT_STRING),
            ("options/aid", mpv::FORMAT_STRING),
            ("options/sid", mpv::FORMAT_STRING),
            ("media-title", mpv::FORMAT_STRING),
            ("path", mpv::FORMAT_STRING),
            ("current-ao", mpv::FORMAT_STRING),
            ("hwdec-current", mpv::FORMAT_STRING),
        ] {
            let name = CString::new(name).map_err(|error| error.to_string())?;
            // SAFETY: The initialized handle and property name remain valid during the call.
            raw.api
                .check(unsafe { (raw.api.observe)(handle, 0, name.as_ptr(), format) })?;
        }
        // SAFETY: Callback storage lives in RawSession until terminate_destroy has joined
        // the core. The callback only queues Qt work and never calls libmpv itself.
        unsafe {
            (raw.api.wakeup)(
                handle,
                Some(crate::controller::wakeup),
                (&*raw._wakeup as *const Wakeup).cast_mut().cast(),
            );
        }
        let generation = raw._wakeup.generation;
        Ok(Arc::new(Self {
            raw: Some(raw),
            next_command: AtomicU64::new(1),
            generation,
        }))
    }

    fn raw(&self) -> &RawSession {
        self.raw
            .as_ref()
            .expect("a live Session owns its native handle")
    }

    pub fn command(&self, arguments: &[&str]) -> Result<u64, String> {
        let strings: Vec<_> = arguments
            .iter()
            .map(|value| CString::new(*value))
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let mut pointers: Vec<_> = strings.iter().map(|value| value.as_ptr()).collect();
        pointers.push(ptr::null());
        let id = self.next_command.fetch_add(1, Ordering::Relaxed);
        let raw = self.raw();
        // SAFETY: mpv copies asynchronous command arguments before returning.
        raw.api
            .check(unsafe { (raw.api.command)(raw.handle, id, pointers.as_ptr()) })?;
        Ok(id)
    }

    pub fn audio_delay(&self) -> Result<f64, String> {
        let raw = self.raw();
        let mut value = 0.0;
        // SAFETY: libmpv writes one double into the provided stack value.
        raw.api.check(unsafe {
            (raw.api.get_property)(
                raw.handle,
                c"audio-delay".as_ptr(),
                mpv::FORMAT_DOUBLE,
                (&mut value as *mut f64).cast(),
            )
        })?;
        Ok(value)
    }

    pub fn poll(&self) -> Option<Event> {
        let raw = self.raw();
        // SAFETY: Only the Qt controller polls this handle. Event data is copied before
        // the next wait_event call; rendering uses its own, distinct API.
        let event = unsafe { &*(raw.api.wait)(raw.handle, 0.0) };
        match event.id {
            mpv::EVENT_NONE => None,
            mpv::EVENT_SHUTDOWN => Some(Event::Shutdown),
            mpv::EVENT_COMMAND_REPLY => Some(Event::Command(event.userdata, event.error)),
            mpv::EVENT_START_FILE => Some(Event::StartFile),
            mpv::EVENT_END_FILE => {
                // SAFETY: END_FILE data has the pinned mpv_event_end_file layout.
                let data = unsafe { &*event.data.cast::<mpv::RawEndFile>() };
                Some(Event::EndFile(data.error))
            }
            mpv::EVENT_FILE_LOADED => Some(Event::FileLoaded),
            mpv::EVENT_PROPERTY_CHANGE => {
                // SAFETY: PROPERTY_CHANGE data has the pinned mpv_event_property layout.
                let data = unsafe { &*event.data.cast::<mpv::RawProperty>() };
                // SAFETY: Property names are static terminated strings supplied by libmpv.
                let name = unsafe { CStr::from_ptr(data.name) }
                    .to_string_lossy()
                    .into_owned();
                let value = if data.data.is_null() {
                    PropertyValue::Unavailable
                } else {
                    // SAFETY: libmpv provides a value matching the requested property format.
                    unsafe {
                        match data.format {
                            mpv::FORMAT_FLAG => PropertyValue::Flag(*data.data.cast::<i32>() != 0),
                            mpv::FORMAT_DOUBLE => PropertyValue::Number(*data.data.cast::<f64>()),
                            mpv::FORMAT_STRING => {
                                let pointer = *data.data.cast::<*const c_char>();
                                PropertyValue::Text(
                                    CStr::from_ptr(pointer).to_string_lossy().into_owned(),
                                )
                            }
                            _ => PropertyValue::Unavailable,
                        }
                    }
                };
                Some(Event::Property(name, value))
            }
            _ => Some(Event::Other),
        }
    }

    pub fn error(&self, code: i32) -> String {
        // SAFETY: libmpv returns a static error string for every client error code.
        unsafe { CStr::from_ptr((self.raw().api.error_string)(code)) }
            .to_string_lossy()
            .into_owned()
    }
}

pub struct RenderLease {
    session: Arc<Session>,
    context: Handle,
    wakeup: Box<FrameWakeup>,
}

impl RenderLease {
    pub fn new(session: Arc<Session>) -> Self {
        let wakeup = Box::new(FrameWakeup {
            thread: session.raw()._wakeup.thread.clone(),
            generation: session.generation,
        });
        Self {
            session,
            context: ptr::null_mut(),
            wakeup,
        }
    }

    pub fn render_generation(&self) -> u64 {
        self.session.generation
    }

    pub fn report_render_error(&self, error: &str) {
        let generation = self.session.generation;
        let error = cxx_qt_lib::QString::from(error);
        let _ = self.wakeup.thread.queue(move |mut player| {
            player.as_mut().surface_failed(generation, &error);
        });
    }

    /// # Safety
    /// The callback must resolve functions for the current OpenGL context. This
    /// lease must remain on that render thread/context until it is dropped.
    pub unsafe fn initialize_render(&mut self, resolver: usize) -> Result<(), String> {
        let raw = self.session.raw();
        // SAFETY: The Qt render hook supplies the documented C callback signature.
        let get_proc_address =
            unsafe { std::mem::transmute::<usize, mpv::OpenGlResolver>(resolver) };
        let mut gl = OpenGlInit {
            get_proc_address,
            context: ptr::null_mut(),
        };
        let mut params = [
            RenderParam {
                kind: mpv::RENDER_API_TYPE,
                data: c"opengl".as_ptr().cast_mut().cast(),
            },
            RenderParam {
                kind: mpv::RENDER_OPENGL_INIT_PARAMS,
                data: (&mut gl as *mut OpenGlInit).cast(),
            },
            RenderParam {
                kind: 0,
                data: ptr::null_mut(),
            },
        ];
        // SAFETY: The current context and session live through this render-only call.
        raw.api.check(unsafe {
            (raw.api.create_render)(&mut self.context, raw.handle, params.as_mut_ptr())
        })?;
        // SAFETY: The callback storage remains owned until render-context destruction.
        unsafe {
            (raw.api.render_wakeup)(
                self.context,
                Some(crate::controller::frame_wakeup),
                (&*self.wakeup as *const FrameWakeup).cast_mut().cast(),
            );
        }
        Ok(())
    }

    /// # Safety
    /// Use the same current OpenGL context that initialized this lease.
    pub unsafe fn draw(&self, framebuffer: i32, width: i32, height: i32) -> Result<(), String> {
        let mut target = OpenGlFbo {
            id: framebuffer,
            width,
            height,
            internal_format: 0,
        };
        let mut params = [
            RenderParam {
                kind: mpv::RENDER_OPENGL_FBO,
                data: (&mut target as *mut OpenGlFbo).cast(),
            },
            RenderParam {
                kind: 0,
                data: ptr::null_mut(),
            },
        ];
        let api = &self.session.raw().api;
        // SAFETY: Context, framebuffer and parameters are owned by the current Qt render thread.
        api.check(unsafe { (api.render)(self.context, params.as_mut_ptr()) })
    }
}

impl Drop for RenderLease {
    fn drop(&mut self) {
        if !self.context.is_null() {
            // SAFETY: Qt releases the renderer on its owning, current OpenGL context.
            unsafe { (self.session.raw().api.free_render)(self.context) };
        }
    }
}

pub fn media_path(input: &str) -> Result<String, String> {
    if input.contains("://") {
        return Ok(input.into());
    }
    let path: PathBuf = Path::new(input)
        .canonicalize()
        .map_err(|error| format!("Open media: {error}"))?;
    Ok(path.to_string_lossy().trim_start_matches("\\\\?\\").into())
}
