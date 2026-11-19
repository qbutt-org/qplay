use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[cxx::bridge(namespace = "Qplay::TensorRt")]
mod ffi {
    extern "Rust" {
        type BuildControl<'a>;
        fn engine_build_progress(control: &BuildControl<'_>, percent: i32);
        fn engine_build_cancelled(control: &BuildControl<'_>) -> bool;
    }

    unsafe extern "C++" {
        include!("qplay_native/tensorrt.h");
        fn build_engine(
            directory: &str,
            resources: &[String],
            model: &str,
            device: u32,
            width: u32,
            height: u32,
            control: &BuildControl<'_>,
        ) -> Result<Vec<u8>>;
        fn validate_engine(
            directory: &str,
            engine: &[u8],
            device: u32,
            width: u32,
            height: u32,
        ) -> Result<()>;
    }
}

pub struct BuildControl<'a> {
    cancel: &'a AtomicBool,
    progress: Mutex<&'a mut (dyn FnMut(i32) + Send)>,
}

fn engine_build_progress(control: &BuildControl<'_>, percent: i32) {
    if let Ok(mut update) = control.progress.lock() {
        update(percent);
    }
}

fn engine_build_cancelled(control: &BuildControl<'_>) -> bool {
    control.cancel.load(Ordering::Acquire)
}

pub fn build(
    directory: &str,
    resources: &[String],
    model: &str,
    device: u32,
    width: u32,
    height: u32,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(i32) + Send),
) -> Result<Vec<u8>, String> {
    ffi::build_engine(
        directory,
        resources,
        model,
        device,
        width,
        height,
        &BuildControl {
            cancel,
            progress: Mutex::new(progress),
        },
    )
    .map_err(|error| format!("Build TensorRT engine: {error}"))
}

pub fn validate(
    directory: &str,
    engine: &[u8],
    device: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    ffi::validate_engine(directory, engine, device, width, height)
        .map_err(|error| format!("Validate TensorRT engine: {error}"))
}
