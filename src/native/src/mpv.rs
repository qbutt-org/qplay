//! ABI subset of include/mpv/{client,render,render_gl}.h at upstream-lock.json's mpv revision.

use std::ffi::{CStr, c_char, c_double, c_int, c_ulong, c_void};
use std::path::Path;

use libloading::os::windows::Library;
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/engine_pin.rs"));

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_DOUBLE: c_int = 5;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_COMMAND_REPLY: c_int = 5;
pub const EVENT_START_FILE: c_int = 6;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

pub const RENDER_API_TYPE: c_int = 1;
pub const RENDER_OPENGL_INIT_PARAMS: c_int = 2;
pub const RENDER_OPENGL_FBO: c_int = 3;

#[repr(C)]
pub struct RawEvent {
    pub id: c_int,
    pub error: c_int,
    pub userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct RawProperty {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct RawEndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub struct RenderParam {
    pub kind: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct OpenGlInit {
    pub get_proc_address: OpenGlResolver,
    pub context: *mut c_void,
}

#[repr(C)]
pub struct OpenGlFbo {
    pub id: c_int,
    pub width: c_int,
    pub height: c_int,
    pub internal_format: c_int,
}

pub type Handle = *mut c_void;
pub type Wakeup = Option<unsafe extern "C" fn(*mut c_void)>;
pub type OpenGlResolver = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;

pub struct Api {
    _library: Library,
    pub create: unsafe extern "C" fn() -> Handle,
    pub initialize: unsafe extern "C" fn(Handle) -> c_int,
    pub terminate: unsafe extern "C" fn(Handle),
    pub option: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    pub get_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub command: unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int,
    pub observe: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    pub wait: unsafe extern "C" fn(Handle, c_double) -> *mut RawEvent,
    pub wakeup: unsafe extern "C" fn(Handle, Wakeup, *mut c_void),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub create_render: unsafe extern "C" fn(*mut Handle, Handle, *mut RenderParam) -> c_int,
    pub render: unsafe extern "C" fn(Handle, *mut RenderParam) -> c_int,
    pub free_render: unsafe extern "C" fn(Handle),
    pub render_wakeup: unsafe extern "C" fn(Handle, Wakeup, *mut c_void),
}

impl Api {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("Read mpv library: {error}"))?;
        if format!("{:x}", Sha256::digest(bytes)) != ENGINE_SHA256 {
            return Err("The mpv library differs from its pinned build.".into());
        }
        let path = path
            .canonicalize()
            .map_err(|error| format!("Locate mpv library: {error}"))?;
        // SAFETY: The validated absolute DLL and its dependencies use the DLL directory and
        // Windows' trusted default directories, never the current directory or caller PATH.
        let library = unsafe { Library::load_with_flags(&path, 0x100 | 0x1000) }
            .map_err(|error| format!("Load mpv library: {error}"))?;
        macro_rules! symbol {
            ($name:literal) => {
                // SAFETY: These signatures match the pinned client/render API headers.
                unsafe { library.get(concat!($name, "\0").as_bytes()) }
                    .map(|symbol| *symbol)
                    .map_err(|error| format!("Load mpv API: {error}"))?
            };
        }
        let version: unsafe extern "C" fn() -> c_ulong = symbol!("mpv_client_api_version");
        // SAFETY: This API has no parameters and the library remains loaded.
        let version = unsafe { version() };
        if version >> 16 != 2 || version & 0xffff < 5 {
            return Err("Unsupported mpv client API.".into());
        }
        Ok(Self {
            create: symbol!("mpv_create"),
            initialize: symbol!("mpv_initialize"),
            terminate: symbol!("mpv_terminate_destroy"),
            option: symbol!("mpv_set_option_string"),
            get_property: symbol!("mpv_get_property"),
            command: symbol!("mpv_command_async"),
            observe: symbol!("mpv_observe_property"),
            wait: symbol!("mpv_wait_event"),
            wakeup: symbol!("mpv_set_wakeup_callback"),
            error_string: symbol!("mpv_error_string"),
            create_render: symbol!("mpv_render_context_create"),
            render: symbol!("mpv_render_context_render"),
            free_render: symbol!("mpv_render_context_free"),
            render_wakeup: symbol!("mpv_render_context_set_update_callback"),
            _library: library,
        })
    }

    pub fn check(&self, result: c_int) -> Result<(), String> {
        if result < 0 {
            // SAFETY: The API returns a static, terminated UTF-8 error string.
            Err(unsafe { CStr::from_ptr((self.error_string)(result)) }
                .to_string_lossy()
                .into_owned())
        } else {
            Ok(())
        }
    }
}
