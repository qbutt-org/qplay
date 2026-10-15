//! ABI subset of include/mpv/{client,render,render_gl}.h at upstream-lock.json's mpv revision.

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_ulong, c_void};
use std::path::Path;

use libloading::os::windows::Library;
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/engine_pin.rs"));

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_DOUBLE: c_int = 5;
const FORMAT_NODE: c_int = 6;
const FORMAT_NODE_ARRAY: c_int = 7;
const FORMAT_NODE_MAP: c_int = 8;

#[repr(C)]
union NodeValue {
    string: *mut c_char,
    list: *mut NodeList,
}

#[repr(C)]
struct Node {
    value: NodeValue,
    format: c_int,
}

#[repr(C)]
struct NodeList {
    count: c_int,
    values: *mut Node,
    keys: *mut *mut c_char,
}

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_LOG_MESSAGE: c_int = 2;
pub const EVENT_COMMAND_REPLY: c_int = 5;
pub const EVENT_START_FILE: c_int = 6;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_CLIENT_MESSAGE: c_int = 16;
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
pub struct RawClientMessage {
    pub count: c_int,
    pub arguments: *const *const c_char,
}

#[repr(C)]
pub struct RawLogMessage {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: c_int,
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
    option_node: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub get_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub free: unsafe extern "C" fn(*mut c_void),
    free_node: unsafe extern "C" fn(*mut Node),
    pub command: unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int,
    pub observe: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    pub request_log_messages: unsafe extern "C" fn(Handle, *const c_char) -> c_int,
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
        let library =
            unsafe { Library::load_with_flags(&path, 0x100 | 0x1000) }.map_err(|error| {
                let system_error = std::io::Error::last_os_error();
                format!("Load mpv library: {error} ({system_error})")
            })?;
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
            option_node: symbol!("mpv_set_option"),
            get_property: symbol!("mpv_get_property"),
            free: symbol!("mpv_free"),
            free_node: symbol!("mpv_free_node_contents"),
            command: symbol!("mpv_command_async"),
            observe: symbol!("mpv_observe_property"),
            request_log_messages: symbol!("mpv_request_log_messages"),
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

    pub fn string_array(&self, handle: Handle, name: &str) -> Option<Vec<String>> {
        let name = CString::new(name).ok()?;
        let mut node = Node {
            value: NodeValue {
                list: std::ptr::null_mut(),
            },
            format: 0,
        };
        // SAFETY: mpv initializes this node and owns its contents until free_node.
        let status = unsafe {
            (self.get_property)(
                handle,
                name.as_ptr(),
                FORMAT_NODE,
                (&mut node as *mut Node).cast(),
            )
        };
        if status < 0 {
            return None;
        }
        let result = (|| {
            if node.format != FORMAT_NODE_ARRAY {
                return None;
            }
            // SAFETY: The pinned client API supplies an array for FORMAT_NODE_ARRAY.
            let list = unsafe { node.value.list.as_ref() }?;
            if !(0..=64).contains(&list.count) || (list.count != 0 && list.values.is_null()) {
                return None;
            }
            let mut entries = Vec::new();
            let mut bytes = 0;
            for index in 0..list.count as usize {
                // SAFETY: index is bounded by the validated client array length.
                let entry = unsafe { &*list.values.add(index) };
                if entry.format != FORMAT_STRING {
                    return None;
                }
                let pointer = unsafe { entry.value.string };
                if pointer.is_null() {
                    return None;
                }
                let value = unsafe { CStr::from_ptr(pointer) }.to_str().ok()?;
                bytes += value.len();
                if bytes > 64 * 1024 {
                    return None;
                }
                entries.push(value.to_owned());
            }
            Some(entries)
        })();
        // SAFETY: Exactly one release follows a successful node property read.
        unsafe { (self.free_node)(&mut node) };
        result
    }

    pub fn set_string_map(
        &self,
        handle: Handle,
        name: &str,
        entries: &[(String, String)],
    ) -> Result<(), String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        let entries = entries
            .iter()
            .map(|(key, value)| Ok((CString::new(key.as_str())?, CString::new(value.as_str())?)))
            .collect::<Result<Vec<_>, std::ffi::NulError>>()
            .map_err(|error| error.to_string())?;
        let mut keys = entries
            .iter()
            .map(|(key, _)| key.as_ptr().cast_mut())
            .collect::<Vec<_>>();
        let mut values = entries
            .iter()
            .map(|(_, value)| Node {
                value: NodeValue {
                    string: value.as_ptr().cast_mut(),
                },
                format: FORMAT_STRING,
            })
            .collect::<Vec<_>>();
        let mut list = NodeList {
            count: entries.len() as c_int,
            values: values.as_mut_ptr(),
            keys: keys.as_mut_ptr(),
        };
        let mut node = Node {
            value: NodeValue { list: &mut list },
            format: FORMAT_NODE_MAP,
        };
        // SAFETY: mpv_set_option copies the node map synchronously. The strings,
        // nodes, and pointer arrays remain alive through this call.
        self.check(unsafe {
            (self.option_node)(
                handle,
                name.as_ptr(),
                FORMAT_NODE,
                (&mut node as *mut Node).cast(),
            )
        })
    }

    pub fn set_string_array(
        &self,
        handle: Handle,
        name: &str,
        entries: &[String],
    ) -> Result<(), String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        let entries = entries
            .iter()
            .map(|entry| CString::new(entry.as_str()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut values = entries
            .iter()
            .map(|entry| Node {
                value: NodeValue {
                    string: entry.as_ptr().cast_mut(),
                },
                format: FORMAT_STRING,
            })
            .collect::<Vec<_>>();
        let mut list = NodeList {
            count: entries.len() as c_int,
            values: values.as_mut_ptr(),
            keys: std::ptr::null_mut(),
        };
        let mut node = Node {
            value: NodeValue { list: &mut list },
            format: FORMAT_NODE_ARRAY,
        };
        // SAFETY: mpv_set_option copies the node array synchronously. The strings
        // and nodes remain alive through this call.
        self.check(unsafe {
            (self.option_node)(
                handle,
                name.as_ptr(),
                FORMAT_NODE,
                (&mut node as *mut Node).cast(),
            )
        })
    }
}
