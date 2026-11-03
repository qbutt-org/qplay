use std::sync::atomic::{AtomicBool, Ordering};

#[cxx::bridge(namespace = "Qplay::TensorRt")]
mod ffi {
    extern "Rust" {
        type BuildControl<'a>;
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
) -> Result<Vec<u8>, String> {
    ffi::build_engine(
        directory,
        resources,
        model,
        device,
        width,
        height,
        &BuildControl { cancel },
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
