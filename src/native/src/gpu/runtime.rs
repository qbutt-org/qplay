use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::framegen::Backend;
use super::process::run_bounded;

const MAX_STATUS_BYTES: u64 = 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
static LOADED_VSSCRIPT: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum State {
    NotInstalled,
    Ready,
    Damaged,
}

#[derive(Deserialize)]
struct Gpu {
    backend: String,
    supported: bool,
}

#[derive(Deserialize)]
struct Status {
    state: State,
    runtime_root: PathBuf,
    version_dir: Option<PathBuf>,
    problems: Vec<String>,
    gpu: Option<Gpu>,
}

#[derive(Deserialize)]
pub struct DeviceBinding {
    pub device_ordinal: u32,
    pub gpu_id: String,
}

#[derive(Deserialize)]
pub struct Runtime {
    schema_version: u32,
    version: u32,
    ready: bool,
    backend: String,
    pub version_dir: PathBuf,
    pub vsscript_dir: PathBuf,
    pub python_home: PathBuf,
    pub plugin_dir: PathBuf,
    pub model_dir: PathBuf,
    pub engine_dir: PathBuf,
    pub device_binding: DeviceBinding,
}

impl Runtime {
    pub fn backend(&self) -> Backend {
        match self.backend.as_str() {
            "trt" => Backend::TensorRt,
            "openvino" => Backend::OpenVino,
            _ => unreachable!("validated runtime backend"),
        }
    }

    pub fn model_path(&self) -> PathBuf {
        self.model_dir.join("rife_v2/rife_v4.26.onnx")
    }

    pub fn vsscript_path(&self) -> PathBuf {
        self.vsscript_dir.join("vsscript.dll")
    }

    pub fn load_vsscript(&self, runtime_root: &Path) -> Result<(), String> {
        let versions = runtime_root
            .join("versions")
            .canonicalize()
            .map_err(|error| format!("Locate managed runtimes: {error}"))?;
        let version = self
            .version_dir
            .canonicalize()
            .map_err(|error| format!("Locate frame generation runtime: {error}"))?;
        if version.parent() != Some(versions.as_path()) {
            return Err("The frame generation runtime is outside its managed directory.".into());
        }
        let expected = version.join("python/Lib/site-packages/vapoursynth/vsscript.dll");
        let actual = self
            .vsscript_path()
            .canonicalize()
            .map_err(|error| format!("Locate VSScript: {error}"))?;
        if actual != expected.canonicalize().map_err(|error| error.to_string())? {
            return Err("VSScript differs from the verified runtime.".into());
        }
        let mut loaded = LOADED_VSSCRIPT
            .lock()
            .map_err(|_| "The VSScript loader is unavailable.")?;
        if let Some(loaded) = loaded.as_ref() {
            return if *loaded == actual {
                Ok(())
            } else {
                Err("A different frame generation runtime is loaded; restart qplay.".into())
            };
        }
        let library =
            unsafe { libloading::os::windows::Library::load_with_flags(&actual, 0x100 | 0x800) }
                .map_err(|error| format!("Load VSScript: {error}"))?;
        std::mem::forget(library); // VSScript and its Python state live until process exit.
        *loaded = Some(actual);
        Ok(())
    }

    pub fn prepare_engine(
        &self,
        script: &Path,
        config: &Value,
        cancel: &AtomicBool,
    ) -> Result<bool, String> {
        if self.backend() == Backend::OpenVino {
            return Ok(true);
        }
        let python = self.python_home.join("python.exe");
        let serialized = serde_json::to_string(config)
            .map_err(|error| format!("Write frame generation configuration: {error}"))?;
        let python = python
            .to_str()
            .ok_or("The Python runtime path is not valid UTF-8.")?;
        let script = script
            .to_str()
            .ok_or("The engine script path is not valid UTF-8.")?;
        let output = run_bounded(
            Path::new(python),
            &["-B", script, &serialized],
            cancel,
            Duration::from_secs(90),
            MAX_STATUS_BYTES,
            None,
        )?;
        let response: Value = serde_json::from_slice(&output)
            .map_err(|error| format!("Read TensorRT engine status: {error}"))?;
        if response.get("ready").and_then(Value::as_bool) == Some(true) {
            return Ok(true);
        }
        run_bounded(
            Path::new(python),
            &["-B", script, &serialized, "--build"],
            cancel,
            Duration::from_secs(30 * 60),
            MAX_STATUS_BYTES,
            None,
        )?;
        Ok(false) // Start a compiled graph only on the next file or explicit choice.
    }
}

pub struct Manager {
    executable: PathBuf,
    runtime_root: PathBuf,
}

impl Manager {
    pub fn new(engine_root: &Path, player_data: &Path) -> Self {
        Self {
            executable: engine_root.join("profile/tools/bin/framegen-manager.exe"),
            runtime_root: player_data.join("framegen"),
        }
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    fn status(&self, cancel: &AtomicBool) -> Result<Status, String> {
        if !self.executable.is_file() {
            return Err("The frame generation installer is missing.".into());
        }
        let bytes = run_bounded(
            &self.executable,
            &["status"],
            cancel,
            Duration::from_secs(30),
            MAX_STATUS_BYTES,
            Some(("QPLAY_MEDIA_RUNTIME", &self.runtime_root)),
        )?;
        let status: Status = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Read frame generation status: {error}"))?;
        if status.runtime_root != self.runtime_root {
            return Err("The installer reported a different runtime directory.".into());
        }
        Ok(status)
    }

    fn verified_runtime(&self, status: &Status) -> Result<Option<Runtime>, String> {
        if !matches!(status.state, State::Ready) || !status.problems.is_empty() {
            return Ok(None);
        }
        let metadata = fs::symlink_metadata(self.runtime_root.join("runtime-manifest.json"))
            .map_err(|error| format!("Inspect frame generation manifest: {error}"))?;
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            return Err("The frame generation manifest has an invalid size.".into());
        }
        let bytes = fs::read(self.runtime_root.join("runtime-manifest.json"))
            .map_err(|error| format!("Read frame generation manifest: {error}"))?;
        let runtime: Runtime = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Parse frame generation manifest: {error}"))?;
        if runtime.schema_version != 3
            || runtime.version != 3
            || !runtime.ready
            || status.version_dir.as_ref() != Some(&runtime.version_dir)
            || !matches!(runtime.backend.as_str(), "trt" | "openvino")
            || status
                .gpu
                .as_ref()
                .is_none_or(|gpu| !gpu.supported || gpu.backend != runtime.backend)
        {
            return Err("The frame generation runtime changed during validation.".into());
        }
        Ok(Some(runtime))
    }

    pub fn inspect(&self, cancel: &AtomicBool) -> Result<Option<Runtime>, String> {
        self.verified_runtime(&self.status(cancel)?)
    }

    pub fn prepare(&self, cancel: &AtomicBool) -> Result<Runtime, String> {
        let status = self.status(cancel)?;
        if let Some(runtime) = self.verified_runtime(&status)? {
            return Ok(runtime);
        }
        if !status.gpu.as_ref().is_some_and(|gpu| gpu.supported) {
            return Err("Frame generation is unavailable on this GPU.".into());
        }
        let command = if matches!(status.state, State::NotInstalled) {
            "install"
        } else {
            "repair"
        };
        run_bounded(
            &self.executable,
            &[command],
            cancel,
            Duration::from_secs(30 * 60),
            MAX_STATUS_BYTES,
            Some(("QPLAY_MEDIA_RUNTIME", &self.runtime_root)),
        )?;
        self.inspect(cancel)?
            .ok_or_else(|| "The frame generation runtime is still unavailable.".into())
    }
}
