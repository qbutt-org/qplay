//! Typed configuration and exact timeline for the native VapourSynth graph.

use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use serde::Deserialize;

use crate::gpu::framegen::{Backend, model_shape};
use crate::gpu::runtime::lock::ExclusiveFileLock;

#[cxx::bridge(namespace = "Qplay::Framegen")]
mod ffi {
    struct GraphConfig {
        tensor_rt: bool,
        target_num: i64,
        target_den: i64,
        step_num: u64,
        step_den: u64,
        scene_threshold: f64,
        duplicate_threshold: f64,
        matrix: i32,
        full_range: bool,
        plugin_path: String,
        model_path: String,
        engine_path: String,
        device_id: u32,
        gpu_uuid: Vec<u8>,
        openvino_device: String,
        gpu_lock_path: String,
    }

    struct Sample {
        index: i32,
        remainder: u64,
    }

    struct ModelShape {
        width: u32,
        height: u32,
    }

    extern "Rust" {
        type GraphLease;
        fn parse_graph_config(config: &str) -> Result<GraphConfig>;
        fn acquire_graph_lease(path: &str) -> Result<Box<GraphLease>>;
        fn sample_position(n: u32, numerator: u64, denominator: u64) -> Sample;
        fn output_length(source_frames: u32, numerator: u64, denominator: u64) -> i32;
        fn inference_shape(width: u32, height: u32, tensor_rt: bool) -> ModelShape;
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u32,
    backend: Backend,
    source_num: u32,
    source_den: u32,
    target_num: u32,
    target_den: u32,
    scene_threshold: f64,
    duplicate_threshold: f64,
    matrix: String,
    range: String,
    plugin_path: String,
    model_path: String,
    engine_path: String,
    device_id: u32,
    gpu_id: String,
    openvino_device: String,
    gpu_lock_path: String,
}

fn parse_graph_config(config: &str) -> Result<ffi::GraphConfig, String> {
    if config.len() > 64 * 1024 {
        return Err("The frame generation configuration is too large.".into());
    }
    let config: Config = serde_json::from_str(config)
        .map_err(|error| format!("Read native frame generation configuration: {error}"))?;
    if config.schema_version != 2 {
        return Err("Unsupported native frame generation configuration.".into());
    }
    if [
        config.source_num,
        config.source_den,
        config.target_num,
        config.target_den,
    ]
    .contains(&0)
    {
        return Err("Frame generation rates must be positive.".into());
    }
    let numerator = u64::from(config.source_num) * u64::from(config.target_den);
    let denominator = u64::from(config.source_den) * u64::from(config.target_num);
    if numerator >= denominator {
        return Err("The frame generation target must exceed the source rate.".into());
    }
    if !config.scene_threshold.is_finite()
        || !(0.0..=1.0).contains(&config.scene_threshold)
        || !config.duplicate_threshold.is_finite()
        || !(0.0..1.0).contains(&config.duplicate_threshold)
    {
        return Err("Invalid frame generation temporal thresholds.".into());
    }
    let matrix = match config.matrix.as_str() {
        "709" => 1,
        "470bg" => 5,
        "240m" => 7,
        "2020ncl" => 9,
        _ => return Err("Unsupported frame generation color matrix.".into()),
    };
    let full_range = match config.range.as_str() {
        "full" => true,
        "limited" => false,
        _ => return Err("Unsupported frame generation color range.".into()),
    };
    let tensor_rt = matches!(config.backend, Backend::TensorRt);
    let gpu_uuid = if tensor_rt {
        let uuid = config
            .gpu_id
            .strip_prefix("GPU-")
            .ok_or("Invalid NVIDIA GPU UUID.")?;
        let digits = uuid.replace('-', "");
        if digits.len() != 32 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Invalid NVIDIA GPU UUID.".into());
        }
        (0..16)
            .map(|index| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).unwrap())
            .collect()
    } else {
        Vec::new()
    };
    for path in [&config.plugin_path, &config.model_path] {
        if !Path::new(path).is_absolute() || !Path::new(path).is_file() || path.contains('\0') {
            return Err("A native frame generation dependency is missing.".into());
        }
    }
    let engine = Path::new(&config.engine_path);
    if !engine.is_absolute()
        || config.engine_path.contains('\0')
        || if tensor_rt {
            !engine.is_file()
        } else {
            !engine.is_dir()
        }
    {
        return Err("The native frame generation engine is not prepared.".into());
    }
    if config.gpu_id.is_empty()
        || !Path::new(&config.gpu_lock_path).is_absolute()
        || config.gpu_lock_path.contains('\0')
    {
        return Err("The native frame generation GPU binding is missing.".into());
    }
    if !tensor_rt
        && config.openvino_device != "GPU"
        && !config
            .openvino_device
            .strip_prefix("GPU.")
            .is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
    {
        return Err("The OpenVINO GPU device is invalid.".into());
    }
    Ok(ffi::GraphConfig {
        tensor_rt,
        target_num: config.target_num.into(),
        target_den: config.target_den.into(),
        step_num: numerator,
        step_den: denominator,
        scene_threshold: config.scene_threshold,
        duplicate_threshold: config.duplicate_threshold,
        matrix,
        full_range,
        plugin_path: config.plugin_path,
        model_path: config.model_path,
        engine_path: config.engine_path,
        device_id: config.device_id,
        gpu_uuid,
        openvino_device: config.openvino_device,
        gpu_lock_path: config.gpu_lock_path,
    })
}

struct GraphLease {
    _lock: Arc<ExclusiveFileLock>,
}

// mpv can construct a replacement graph before releasing its predecessor.
// Share that process's lease until the last graph has freed its GPU resources.
static GRAPH_LEASE: Mutex<Option<(PathBuf, Weak<ExclusiveFileLock>)>> = Mutex::new(None);

fn acquire_graph_lease(path: &str) -> Result<Box<GraphLease>, String> {
    let path = Path::new(path);
    let mut active = GRAPH_LEASE
        .lock()
        .map_err(|_| "The frame generation GPU lease is unavailable.")?;
    if let Some((active_path, lock)) = active.as_ref()
        && let Some(lock) = lock.upgrade()
    {
        if active_path != path {
            return Err("A different frame generation runtime is still active.".into());
        }
        return Ok(Box::new(GraphLease { _lock: lock }));
    }
    let lock = Arc::new(ExclusiveFileLock::acquire(path)?);
    *active = Some((path.to_owned(), Arc::downgrade(&lock)));
    Ok(Box::new(GraphLease { _lock: lock }))
}

fn sample_position(n: u32, numerator: u64, denominator: u64) -> ffi::Sample {
    let position = u128::from(n) * u128::from(numerator);
    let denominator = u128::from(denominator);
    ffi::Sample {
        // Target > source and VS uses signed 32-bit frame indexes.
        index: (position / denominator) as i32,
        remainder: (position % denominator) as u64,
    }
}

fn output_length(source_frames: u32, numerator: u64, denominator: u64) -> i32 {
    let length =
        (u128::from(source_frames) * u128::from(denominator)).div_ceil(u128::from(numerator));
    length.min(i32::MAX as u128) as i32
}

fn inference_shape(width: u32, height: u32, tensor_rt: bool) -> ffi::ModelShape {
    let backend = if tensor_rt {
        Backend::TensorRt
    } else {
        Backend::OpenVino
    };
    let shape = model_shape(backend, width, height);
    ffi::ModelShape {
        width: shape.width,
        height: shape.height,
    }
}

pub type GraphFactory = unsafe extern "C" fn(
    *const c_void,
    *mut c_void,
    *mut c_void,
    *const c_char,
    *mut c_char,
    usize,
) -> *mut c_void;

unsafe extern "C" {
    pub fn qplay_create_framegen_graph(
        api: *const c_void,
        core: *mut c_void,
        input: *mut c_void,
        config: *const c_char,
        error: *mut c_char,
        error_size: usize,
    ) -> *mut c_void;
}
