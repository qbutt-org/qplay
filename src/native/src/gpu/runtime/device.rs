use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Device {
    pub backend: String,
    pub ordinal: u32,
    pub gpu_id: String,
    pub driver_version: String,
    pub capability: String,
    pub adapter_luid: Option<String>,
}

#[cfg(any(windows, target_os = "linux"))]
fn cuda() -> Result<Option<Device>, String> {
    #[cfg(windows)]
    let (library, path): (libloading::Library, std::path::PathBuf) = {
        use libloading::os::windows::Library;
        let library = match unsafe { Library::load_with_flags("nvcuda.dll", 0x800) } {
            Ok(library) => library,
            Err(_) => return Ok(None),
        };
        let handle = library.into_raw();
        let library = unsafe { Library::from_raw(handle) };
        #[link(name = "Kernel32")]
        unsafe extern "system" {
            fn GetModuleFileNameW(
                module: *mut std::ffi::c_void,
                filename: *mut u16,
                size: u32,
            ) -> u32;
        }
        let mut filename = vec![0_u16; 32768];
        let length = unsafe {
            GetModuleFileNameW(
                handle as *mut std::ffi::c_void,
                filename.as_mut_ptr(),
                filename.len() as u32,
            )
        } as usize;
        if length == 0 || length >= filename.len() {
            return Err("CUDA did not report its loaded driver path.".into());
        }
        use std::os::windows::ffi::OsStringExt;
        let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&filename[..length]));
        (library.into(), path)
    };
    #[cfg(target_os = "linux")]
    let (library, path) = {
        use std::os::unix::ffi::OsStrExt;
        let library = match unsafe { libloading::Library::new("libcuda.so.1") } {
            Ok(library) => library,
            Err(_) => return Ok(None),
        };
        let path = unsafe {
            let init = library
                .get::<unsafe extern "C" fn(u32) -> i32>(b"cuInit\0")
                .map_err(|error| error.to_string())?;
            let mut info: libc::Dl_info = std::mem::zeroed();
            if libc::dladdr(*init as *const () as *const std::ffi::c_void, &mut info) == 0
                || info.dli_fname.is_null()
            {
                return Err("CUDA did not report its loaded driver path.".into());
            }
            let filename = std::ffi::CStr::from_ptr(info.dli_fname);
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(filename.to_bytes()))
        };
        (library, path)
    };
    let driver_hash = crate::files::hash_file(&path)?;
    unsafe {
        let init = library
            .get::<unsafe extern "system" fn(u32) -> i32>(b"cuInit\0")
            .map_err(|error| error.to_string())?;
        let count = library
            .get::<unsafe extern "system" fn(*mut i32) -> i32>(b"cuDeviceGetCount\0")
            .map_err(|error| error.to_string())?;
        let get = library
            .get::<unsafe extern "system" fn(*mut i32, i32) -> i32>(b"cuDeviceGet\0")
            .map_err(|error| error.to_string())?;
        let uuid = library
            .get::<unsafe extern "system" fn(*mut [u8; 16], i32) -> i32>(b"cuDeviceGetUuid\0")
            .map_err(|error| error.to_string())?;
        let memory = library
            .get::<unsafe extern "system" fn(*mut usize, i32) -> i32>(b"cuDeviceTotalMem_v2\0")
            .map_err(|error| error.to_string())?;
        let attr = library
            .get::<unsafe extern "system" fn(*mut i32, i32, i32) -> i32>(b"cuDeviceGetAttribute\0")
            .map_err(|error| error.to_string())?;
        let version = library
            .get::<unsafe extern "system" fn(*mut i32) -> i32>(b"cuDriverGetVersion\0")
            .map_err(|error| error.to_string())?;
        let status = init(0);
        if status == 100 {
            return Ok(None);
        }
        if status != 0 {
            return Err(format!("Initialize CUDA: error {status}."));
        }
        let mut devices = 0;
        if count(&mut devices) != 0 || !(1..=32).contains(&devices) {
            return Ok(None);
        }
        let mut driver_version = 0;
        if version(&mut driver_version) != 0 {
            return Err("CUDA did not report its driver version.".into());
        }
        let mut selected = None;
        for ordinal in 0..devices {
            let mut device = 0;
            let mut id = [0_u8; 16];
            let mut bytes = 0;
            let mut major = 0;
            let mut minor = 0;
            if get(&mut device, ordinal) != 0
                || uuid(&mut id, device) != 0
                || memory(&mut bytes, device) != 0
                || attr(&mut major, 75, device) != 0
                || attr(&mut minor, 76, device) != 0
            {
                return Err("CUDA could not inspect the selected GPU.".into());
            }
            let capability = format!("{major}.{minor}");
            if !matches!(
                capability.as_str(),
                "7.5" | "8.0" | "8.6" | "8.9" | "9.0" | "10.0" | "12.0"
            ) || id == [0; 16]
            {
                continue;
            }
            let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
            let gpu_id = format!(
                "GPU-{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            );
            let candidate = Device {
                backend: "trt".into(),
                ordinal: ordinal as u32,
                gpu_id,
                driver_version: format!(
                    "cuda:{driver_version};{}-sha256:{driver_hash}",
                    if cfg!(windows) { "nvcuda" } else { "libcuda" }
                ),
                capability,
                adapter_luid: None,
            };
            if selected
                .as_ref()
                .is_none_or(|(previous, _)| bytes > *previous)
            {
                selected = Some((bytes, candidate));
            }
        }
        Ok(selected.map(|(_, device)| device))
    }
}

#[cfg(windows)]
pub fn discover() -> Result<Device, String> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIFactory1};
    if std::env::var_os("CUDA_VISIBLE_DEVICES").is_some() {
        return Err("CUDA_VISIBLE_DEVICES must be unset for native frame generation.".into());
    }
    if let Some(device) = cuda()? {
        return Ok(device);
    }
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|error| error.to_string())?;
    for ordinal in 0..32 {
        let adapter = match unsafe { factory.EnumAdapters1(ordinal) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error.to_string()),
        };
        let description = unsafe { adapter.GetDesc1() }.map_err(|error| error.to_string())?;
        if description.VendorId == 0x8086 && description.Flags & 2 == 0 {
            let mut bytes = Vec::from(description.AdapterLuid.LowPart.to_le_bytes());
            bytes.extend_from_slice(&description.AdapterLuid.HighPart.to_le_bytes());
            let luid = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            return Ok(Device {
                backend: "openvino".into(),
                ordinal: 0,
                gpu_id: String::new(),
                driver_version: String::new(),
                capability: "intel-gpu".into(),
                adapter_luid: Some(luid),
            });
        }
    }
    Err("Frame generation is unavailable on this GPU.".into())
}

#[cfg(target_os = "linux")]
pub fn discover() -> Result<Device, String> {
    if std::env::var_os("CUDA_VISIBLE_DEVICES").is_some() {
        return Err("CUDA_VISIBLE_DEVICES must be unset for native frame generation.".into());
    }
    cuda()?.ok_or_else(|| "Frame generation is unavailable on this GPU.".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn discover() -> Result<Device, String> {
    Err("Native frame generation is unavailable on this platform.".into())
}

#[cfg(windows)]
pub fn resolve_openvino(library: &std::path::Path, device: &mut Device) -> Result<String, String> {
    use std::ffi::{CStr, CString, c_char, c_void};
    #[repr(C)]
    struct AvailableDevices {
        devices: *mut *mut c_char,
        size: usize,
    }
    let library =
        unsafe { libloading::os::windows::Library::load_with_flags(library, 0x100 | 0x1000) }
            .map_err(|error| format!("Load native OpenVINO: {error}"))?;
    unsafe {
        let create = library
            .get::<unsafe extern "C" fn(*mut *mut c_void) -> i32>(b"ov_core_create\0")
            .map_err(|error| error.to_string())?;
        let free = library
            .get::<unsafe extern "C" fn(*mut c_void)>(b"ov_core_free\0")
            .map_err(|error| error.to_string())?;
        let available = library
            .get::<unsafe extern "C" fn(*mut c_void, *mut AvailableDevices) -> i32>(
                b"ov_core_get_available_devices\0",
            )
            .map_err(|error| error.to_string())?;
        let free_devices = library
            .get::<unsafe extern "C" fn(*mut AvailableDevices)>(b"ov_available_devices_free\0")
            .map_err(|error| error.to_string())?;
        let property =
            library
                .get::<unsafe extern "C" fn(
                    *mut c_void,
                    *const c_char,
                    *const c_char,
                    *mut *mut c_char,
                ) -> i32>(b"ov_core_get_property\0")
                .map_err(|error| error.to_string())?;
        let free_string = library
            .get::<unsafe extern "C" fn(*mut c_void)>(b"ov_free\0")
            .map_err(|error| error.to_string())?;
        let mut core = std::ptr::null_mut();
        if create(&mut core) != 0 || core.is_null() {
            return Err("OpenVINO could not create its native core.".into());
        }
        let result = (|| {
            let mut list = AvailableDevices {
                devices: std::ptr::null_mut(),
                size: 0,
            };
            if available(core, &mut list) != 0 {
                return Err("OpenVINO could not enumerate GPU devices.".into());
            }
            let names = if list.size <= 32 && (list.size == 0 || !list.devices.is_null()) {
                (0..list.size)
                    .map(|index| {
                        CStr::from_ptr(*list.devices.add(index))
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<Vec<_>>()
            } else {
                free_devices(&mut list);
                return Err("OpenVINO reported an invalid device count.".into());
            };
            free_devices(&mut list);
            let mut matched = None;
            for name in names
                .into_iter()
                .filter(|name| name == "GPU" || name.starts_with("GPU."))
            {
                let device_name = CString::new(name.as_str()).map_err(|error| error.to_string())?;
                let prop = |key: &CStr| -> Result<String, String> {
                    let mut value = std::ptr::null_mut();
                    if property(core, device_name.as_ptr(), key.as_ptr(), &mut value) != 0
                        || value.is_null()
                    {
                        return Err("OpenVINO omitted a GPU identity property.".into());
                    }
                    let result = CStr::from_ptr(value).to_string_lossy().trim().to_owned();
                    free_string(value.cast());
                    Ok(result)
                };
                if prop(c"DEVICE_LUID")?.to_ascii_lowercase()
                    != device.adapter_luid.as_deref().unwrap_or("")
                {
                    continue;
                }
                let uuid = prop(c"DEVICE_UUID")?.to_ascii_lowercase();
                let ordinal = prop(c"DEVICE_ID")?
                    .parse::<u32>()
                    .map_err(|error| error.to_string())?;
                if uuid.len() != 32
                    || !uuid.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || uuid.bytes().all(|byte| byte == b'0')
                    || ordinal >= 32
                    || matched.is_some()
                {
                    return Err("The Intel GPU identity is ambiguous or invalid.".into());
                }
                matched = Some((name, ordinal, uuid, prop(c"DRIVER_VERSION")?));
            }
            let (name, ordinal, uuid, driver) =
                matched.ok_or("The selected Intel adapter is unavailable to OpenVINO.")?;
            device.ordinal = ordinal;
            device.gpu_id = format!("uuid:{uuid}");
            device.driver_version = driver;
            Ok(name)
        })();
        free(core);
        result
    }
}

#[cfg(not(windows))]
pub fn resolve_openvino(_: &std::path::Path, _: &mut Device) -> Result<String, String> {
    Err("Native OpenVINO is unavailable on this platform.".into())
}
