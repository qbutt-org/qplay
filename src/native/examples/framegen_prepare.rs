//! Developer integration entry for the application's native preparation path.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use qplay_native::framegen::{self, Backend, Rate, Selection, Target};
use qplay_native::runtime::{Runtime, RuntimeStore};
use sha2::{Digest, Sha256};

fn engine_files(root: &Path) -> Result<BTreeSet<(PathBuf, u64, String)>, String> {
    let mut files = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                files.insert((entry.path(), 0, "directory".into()));
                pending.push(entry.path());
            } else if kind.is_file() {
                let mut file =
                    std::fs::File::open(entry.path()).map_err(|error| error.to_string())?;
                let mut digest = Sha256::new();
                let mut buffer = [0_u8; 65536];
                loop {
                    let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
                    if count == 0 {
                        break;
                    }
                    digest.update(&buffer[..count]);
                }
                files.insert((
                    entry.path(),
                    entry.metadata().map_err(|error| error.to_string())?.len(),
                    format!("{:x}", digest.finalize()),
                ));
            } else {
                return Err("Unexpected file type in the isolated engine cache.".into());
            }
        }
    }
    Ok(files)
}

fn tensor_rt_modules() -> Result<BTreeSet<String>, String> {
    #[cfg(windows)]
    let paths = {
        use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
            TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        };
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, std::process::id())
        }
        .map_err(|error| error.to_string())?;
        let mut entry = MODULEENTRY32W {
            dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let mut paths = Vec::new();
        let mut status = unsafe { Module32FirstW(snapshot, &mut entry) };
        while status.is_ok() {
            let length = entry
                .szExePath
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExePath.len());
            paths.push(String::from_utf16_lossy(&entry.szExePath[..length]));
            status = unsafe { Module32NextW(snapshot, &mut entry) };
        }
        unsafe { CloseHandle(snapshot) }.map_err(|error| error.to_string())?;
        if status.unwrap_err().code() != windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
            return Err("Module enumeration failed before reaching its end.".into());
        }
        paths
    };
    #[cfg(target_os = "linux")]
    let paths: Vec<String> = std::fs::read_to_string("/proc/self/maps")
        .map_err(|error| error.to_string())?
        .lines()
        .filter_map(|line| line.find('/').map(|start| line[start..].to_owned()))
        .collect();
    Ok(paths
        .into_iter()
        .filter(|path| {
            let name = path.to_ascii_lowercase();
            name.contains("nvinfer") || name.contains("nvonnxparser")
        })
        .collect())
}

fn primary_context_active(ordinal: u32) -> Result<bool, String> {
    #[cfg(windows)]
    let path = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing SystemRoot.")?)
        .join("System32/nvcuda.dll");
    #[cfg(target_os = "linux")]
    let path = PathBuf::from("libcuda.so.1");
    // The driver query neither retains nor creates a primary CUDA context.
    unsafe {
        let library = libloading::Library::new(path).map_err(|error| error.to_string())?;
        let init: libloading::Symbol<unsafe extern "system" fn(u32) -> i32> = library
            .get(b"cuInit\0")
            .map_err(|error| error.to_string())?;
        let get_device: libloading::Symbol<unsafe extern "system" fn(*mut i32, i32) -> i32> =
            library
                .get(b"cuDeviceGet\0")
                .map_err(|error| error.to_string())?;
        let state: libloading::Symbol<unsafe extern "system" fn(i32, *mut u32, *mut i32) -> i32> =
            library
                .get(b"cuDevicePrimaryCtxGetState\0")
                .map_err(|error| error.to_string())?;
        let mut flags = 0;
        let mut active = 0;
        let mut device = 0;
        if init(0) != 0
            || get_device(&mut device, ordinal as i32) != 0
            || state(device, &mut flags, &mut active) != 0
        {
            return Err("Inspect the selected CUDA primary context failed.".into());
        }
        Ok(active != 0)
    }
}

fn with_current_context<T>(
    ordinal: u32,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    #[cfg(windows)]
    let path = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing SystemRoot.")?)
        .join("System32/nvcuda.dll");
    #[cfg(target_os = "linux")]
    let path = PathBuf::from("libcuda.so.1");
    // Hold an independent caller context across the actual preparation call.
    // Cached preparation must restore it after validating the real engine.
    unsafe {
        let library = libloading::Library::new(path).map_err(|error| error.to_string())?;
        let get_device: libloading::Symbol<unsafe extern "system" fn(*mut i32, i32) -> i32> =
            library
                .get(b"cuDeviceGet\0")
                .map_err(|error| error.to_string())?;
        let create: libloading::Symbol<
            unsafe extern "system" fn(*mut *mut std::ffi::c_void, u32, i32) -> i32,
        > = library
            .get(b"cuCtxCreate_v2\0")
            .map_err(|error| error.to_string())?;
        let current: libloading::Symbol<
            unsafe extern "system" fn(*mut *mut std::ffi::c_void) -> i32,
        > = library
            .get(b"cuCtxGetCurrent\0")
            .map_err(|error| error.to_string())?;
        let destroy: libloading::Symbol<unsafe extern "system" fn(*mut std::ffi::c_void) -> i32> =
            library
                .get(b"cuCtxDestroy_v2\0")
                .map_err(|error| error.to_string())?;
        let mut device = 0;
        let mut previous = std::ptr::null_mut();
        let mut caller = std::ptr::null_mut();
        if get_device(&mut device, ordinal as i32) != 0 || current(&mut previous) != 0 {
            return Err("Inspect the caller CUDA context failed.".into());
        }
        if create(&mut caller, 0, device) != 0 || caller.is_null() {
            return Err("Create the caller CUDA context failed.".into());
        }
        let result = operation();
        let mut restored = std::ptr::null_mut();
        let preserved = current(&mut restored) == 0 && restored == caller;
        let destroyed = destroy(caller) == 0;
        let returned = current(&mut restored) == 0 && restored == previous;
        if !preserved || !destroyed || !returned {
            return Err(format!(
                "CUDA context stack changed: caller_preserved={preserved}, caller_destroyed={destroyed}, previous_restored={returned}"
            ));
        }
        result
    }
}

fn handoff(
    mut runtime: Runtime,
    width: u32,
    height: u32,
    delay: Duration,
) -> Result<Runtime, String> {
    if runtime.backend() != Backend::TensorRt {
        return Err("The handoff fixture requires a real TensorRT device.".into());
    }
    let cold_engine = runtime
        .engine_dir
        .join(format!("rife426-{width}x{height}.engine"));
    if cold_engine.exists() || !runtime.engine_dir.join("rife426-641x359.engine").is_file() {
        return Err(
            "Handoff requires an uncached requested geometry and the accepted 641x359 cache."
                .into(),
        );
    }
    let ordinal = runtime.device_binding.ordinal;
    with_current_context(ordinal, || {
        runtime.prepare_engine(641, 359, &AtomicBool::new(false))
    })?;
    let mut successor: Runtime =
        serde_json::from_value(serde_json::to_value(&runtime).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let before_files = engine_files(&runtime.engine_dir)?;
    let before_modules = tensor_rt_modules()?;
    let before_context = primary_context_active(ordinal)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let started = Instant::now();
    let worker = std::thread::spawn(move || {
        let mut runtime = runtime;
        let result = runtime.prepare_engine(width, height, &worker_cancel);
        (result, Instant::now())
    });
    std::thread::sleep(delay);
    let canceled_at = Instant::now();
    cancel.store(true, Ordering::Release);
    let successor_started = Instant::now();
    let successor_result = successor.prepare_engine(641, 359, &AtomicBool::new(false));
    let successor_elapsed = successor_started.elapsed();
    let (canceled_result, finished_at) = worker
        .join()
        .map_err(|_| "The native preparation worker panicked.")?;
    let reuse_result = successor.prepare_engine(641, 359, &AtomicBool::new(false));
    let after_files = engine_files(&successor.engine_dir)?;
    let after_modules = tensor_rt_modules()?;
    let after_context = primary_context_active(ordinal)?;

    #[cfg(windows)]
    let repeat = serde_json::Value::Null;
    #[cfg(target_os = "linux")]
    let repeat = {
        let library_dir = successor.version_dir.join("lib");
        for added in after_modules.difference(&before_modules) {
            let path = Path::new(added);
            if path.parent() != Some(library_dir.as_path())
                || !path.is_file()
                || !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("libnvinfer_builder_resource_"))
            {
                return Err(format!(
                    "An unexpected TensorRT mapping survived preparation: {added}"
                ));
            }
        }
        let mut repeated: Runtime = serde_json::from_value(
            serde_json::to_value(&successor).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let repeat_cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = repeat_cancel.clone();
        let worker =
            std::thread::spawn(move || repeated.prepare_engine(width, height, &worker_cancel));
        std::thread::sleep(delay);
        let requested = Instant::now();
        repeat_cancel.store(true, Ordering::Release);
        let cached_result = successor.prepare_engine(641, 359, &AtomicBool::new(false));
        let canceled = worker
            .join()
            .map_err(|_| "The repeated builder panicked.")?;
        let modules = tensor_rt_modules()?;
        let active = primary_context_active(ordinal)?;
        if !canceled
            .as_ref()
            .is_err_and(|error| error.to_ascii_lowercase().contains("cancel"))
            || cached_result.is_err()
            || modules != after_modules
            || active
            || engine_files(&successor.engine_dir)? != before_files
        {
            return Err(format!(
                "Repeated native teardown was not bounded: canceled={canceled:?}, cached={cached_result:?}, context_active={active}, modules={modules:?}"
            ));
        }
        serde_json::json!({"canceled_result": canceled, "cached_result": cached_result,
            "drain_and_reuse_ms": requested.elapsed().as_millis(),
            "modules": modules, "context_active": active})
    };
    println!(
        "{}",
        serde_json::json!({
            "mode": "handoff", "cold_width": width, "cold_height": height,
            "cancel_requested_ms": canceled_at.duration_since(started).as_millis(),
            "cancel_drain_ms": finished_at.saturating_duration_since(canceled_at).as_millis(),
            "successor_elapsed_ms": successor_elapsed.as_millis(),
            "canceled_result": canceled_result, "successor_result": successor_result,
            "reuse_result": reuse_result, "files_before": before_files, "files_after": after_files,
            "modules_before": before_modules, "modules_after": after_modules,
            "context_active_before": before_context, "context_active_after": after_context,
            "repeat": repeat, "cached_caller_context_preserved": true,
            "elapsed_ms": started.elapsed().as_millis(),
        })
    );
    if !canceled_result
        .as_ref()
        .is_err_and(|error| error.to_ascii_lowercase().contains("cancel"))
    {
        return Err("The cold native builder did not acknowledge cancellation.".into());
    }
    if before_files != after_files || cold_engine.exists() {
        return Err("Canceled preparation changed the accepted engine cache.".into());
    }
    #[cfg(windows)]
    if before_modules != after_modules {
        return Err("Native preparation retained TensorRT modules beyond its scope.".into());
    }
    if before_context || after_context {
        return Err("Native preparation retained a primary CUDA context.".into());
    }
    reuse_result.map_err(|error| format!("GPU lock/cache reuse after drain: {error}"))?;
    successor_result.map_err(|error| format!("Immediate successor preparation: {error}"))?;
    Ok(successor)
}

fn main() -> Result<(), String> {
    cxx_qt::init_crate!(qplay_native);
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if !(5..=7).contains(&arguments.len()) {
        return Err("Usage: framegen_prepare <engine directory> <isolated runtime> <width> <height> <output filter> [cancel after ms | --handoff cancel-after-ms]".into());
    }
    let mut width = arguments[2].parse().map_err(|_| "Invalid source width.")?;
    let mut height = arguments[3].parse().map_err(|_| "Invalid source height.")?;
    let handoff_mode = arguments.get(5).is_some_and(|value| value == "--handoff");
    if arguments.len() == 7 && !handoff_mode || arguments.len() == 6 && handoff_mode {
        return Err("Invalid preparation acceptance mode.".into());
    }
    let delay = arguments
        .get(if handoff_mode { 6 } else { 5 })
        .map(|value| {
            value
                .parse::<u64>()
                .map(Duration::from_millis)
                .map_err(|_| "Invalid cancellation delay.")
        })
        .transpose()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let store = RuntimeStore::new(&PathBuf::from(&arguments[0]), &PathBuf::from(&arguments[1]));
    let mut runtime = store.prepare(&cancel)?;
    if handoff_mode {
        runtime = handoff(runtime, width, height, delay.unwrap())?;
        (width, height) = (641, 359);
    } else {
        // Start cancellation after runtime acquisition so it exercises the builder.
        let cancellation = delay.map(|delay| {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                cancel.store(true, Ordering::Release);
            })
        });
        let result = runtime.prepare_engine(width, height, &cancel);
        if let Some(cancellation) = cancellation {
            cancellation
                .join()
                .map_err(|_| "The cancellation fixture thread failed.")?;
        }
        if let Err(error) = result {
            eprintln!(
                "Native preparation ended after {} ms: {error}",
                started.elapsed().as_millis()
            );
            return Err(error);
        }
    }
    let source = Rate::new(24000, 1001).unwrap();
    let target = Target {
        rate: Rate::new(60000, 1001).unwrap(),
        multiplier: None,
    };
    let filter = framegen::graph(
        &runtime,
        source,
        Selection { target },
        "bt.709",
        false,
        width,
    )?;
    std::fs::write(&arguments[4], filter)
        .map_err(|error| format!("Write native filter fixture: {error}"))?;
    println!(
        "{}",
        serde_json::json!({
            "core": runtime.core_path(), "plugin": runtime.plugin_path(),
            "engine": runtime.engine_path()?, "model": runtime.model_path(),
            "width": width, "height": height, "elapsed_ms": started.elapsed().as_millis(),
        })
    );
    Ok(())
}
