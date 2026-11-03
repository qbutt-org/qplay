mod archive;
mod assets;
mod device;
pub mod lock;
mod notices;
mod storage;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::framegen::Backend;
use crate::tensorrt;
use assets::{ArchiveKind, Asset};
use lock::ExclusiveFileLock;
use storage::{Staging, collect_files, normal_directory, normal_file, reject_links, safe_relative};

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));
// Revise when the native layout, tensor or builder precision contract changes.
const RUNTIME_ABI: &[u8] = b"qplay-native-framegen-1";
const MANIFEST: &str = "native-runtime-manifest.json";

#[derive(Deserialize, Serialize)]
struct ManifestFile {
    size: u64,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
pub struct Runtime {
    runtime_id: String,
    pub version_dir: PathBuf,
    pub engine_dir: PathBuf,
    pub gpu_lock_path: PathBuf,
    pub device_binding: device::Device,
    pub openvino_device: String,
    files: BTreeMap<String, ManifestFile>,
    #[serde(skip)]
    prepared_engine: Option<PathBuf>,
}

impl Runtime {
    pub fn backend(&self) -> Backend {
        match self.device_binding.backend.as_str() {
            "trt" => Backend::TensorRt,
            "openvino" => Backend::OpenVino,
            _ => unreachable!("validated native runtime backend"),
        }
    }

    pub fn core_path(&self) -> PathBuf {
        self.version_dir.join(if cfg!(windows) {
            "vapoursynth/libvapoursynth.dll"
        } else {
            "lib/libvapoursynth.so.4"
        })
    }

    pub fn model_path(&self) -> PathBuf {
        self.version_dir.join("models/rife_v2/rife_v4.26.onnx")
    }

    pub fn plugin_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        return self.version_dir.join("lib/libvstrt.so");
        #[cfg(windows)]
        self.version_dir.join(match self.backend() {
            Backend::TensorRt => "plugins/vstrt.dll",
            Backend::OpenVino => "plugins/vsov.dll",
        })
    }

    fn library_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        return self.version_dir.join("lib");
        #[cfg(windows)]
        self.version_dir.join(match self.backend() {
            Backend::TensorRt => "plugins/vsmlrt-cuda",
            Backend::OpenVino => "plugins/vsov",
        })
    }

    pub fn engine_path(&self) -> Result<&Path, String> {
        match self.backend() {
            Backend::TensorRt => self
                .prepared_engine
                .as_deref()
                .ok_or("The TensorRT engine has not been prepared.".into()),
            Backend::OpenVino => Ok(&self.engine_dir),
        }
    }

    /// Called by the Qt pool worker. TensorRT unwinds on cooperative cancellation
    /// before this scope releases its context, libraries and exclusive GPU lock.
    pub fn prepare_engine(
        &mut self,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        check_cancel(cancel)?;
        if self.backend() == Backend::OpenVino {
            return Ok(());
        }
        let shape = super::framegen::model_shape(self.backend(), width, height);
        let (width, height) = (shape.width, shape.height);
        let _gpu = ExclusiveFileLock::wait(&self.gpu_lock_path, cancel)?;
        let target = self
            .engine_dir
            .join(format!("rife426-{width}x{height}.engine"));
        let library_dir = self.library_dir();
        let library = library_dir
            .to_str()
            .ok_or("The TensorRT directory is not valid UTF-8.")?;
        if target.exists() {
            normal_file(&target)?;
            let bytes =
                fs::read(&target).map_err(|error| format!("Read TensorRT engine: {error}"))?;
            tensorrt::validate(library, &bytes, self.device_binding.ordinal, width, height)?;
            check_cancel(cancel)?;
            self.prepared_engine = Some(target);
            return Ok(());
        }
        let model = self.model_path();
        let resources: Vec<String> = assets::tensorrt_resources(&self.device_binding.capability)
            .iter()
            .map(|name| format!("{library}/{name}"))
            .collect();
        let bytes = tensorrt::build(
            library,
            &resources,
            model
                .to_str()
                .ok_or("The RIFE model path is not valid UTF-8.")?,
            self.device_binding.ordinal,
            width,
            height,
            cancel,
        )?;
        check_cancel(cancel)?;
        let stage = Staging::directory(&self.engine_dir)?;
        let partial = stage.path.join("rife.engine");
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|error| format!("Stage TensorRT engine: {error}"))?;
        output
            .write_all(&bytes)
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("Write TensorRT engine: {error}"))?;
        drop(output);
        check_cancel(cancel)?;
        crate::files::move_new_file(&partial, &target)
            .map_err(|error| format!("Publish TensorRT engine: {error}"))?;
        self.prepared_engine = Some(target);
        Ok(())
    }
}

pub struct RuntimeStore {
    engine_root: PathBuf,
    root: PathBuf,
}

impl RuntimeStore {
    pub fn new(engine_root: &Path, runtime_root: &Path) -> Self {
        Self {
            engine_root: engine_root.to_path_buf(),
            root: runtime_root.to_path_buf(),
        }
    }

    fn inspect(
        &self,
        detected: &device::Device,
        runtime_id: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<Runtime>, String> {
        let path = self.root.join(MANIFEST);
        if !path.exists() {
            return Ok(None);
        }
        normal_file(&path)?;
        if fs::metadata(&path)
            .map_err(|error| error.to_string())?
            .len()
            > 1024 * 1024
        {
            return Err("The native GPU runtime manifest exceeds its size limit.".into());
        }
        let runtime: Runtime =
            serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("Read native GPU runtime: {error}"))?;
        if runtime.runtime_id != runtime_id || runtime.device_binding.backend != detected.backend {
            return Ok(None);
        }
        if runtime.version_dir.parent() != Some(self.root.join("versions").as_path())
            || runtime.engine_dir.parent() != Some(self.root.join("engines").as_path())
            || runtime.gpu_lock_path != self.gpu_lock_path()?
        {
            return Err("The native GPU runtime is outside its managed directory.".into());
        }
        normal_directory(&runtime.version_dir)?;
        normal_directory(&runtime.engine_dir)?;
        for (relative, file) in &runtime.files {
            check_cancel(cancel)?;
            safe_relative(relative)?;
            let path = runtime.version_dir.join(relative);
            reject_links(&runtime.version_dir, &path)?;
            if !crate::files::verified_file(&path, file.size, &file.sha256)? {
                return Ok(None);
            }
        }
        if required_paths(runtime.backend(), &detected.capability)
            .iter()
            .any(|name| !runtime.files.contains_key(name))
        {
            return Ok(None);
        }
        let mut current = detected.clone();
        if runtime.backend() == Backend::OpenVino {
            let name = device::resolve_openvino(
                &runtime.library_dir().join("openvino_c.dll"),
                &mut current,
            )?;
            if name != runtime.openvino_device {
                return Ok(None);
            }
        }
        Ok((current == runtime.device_binding).then_some(runtime))
    }

    fn gpu_lock_path(&self) -> Result<PathBuf, String> {
        Ok(self
            .root
            .parent()
            .ok_or("The GPU runtime has no parent directory.")?
            .join("locks/framegen-gpu.lock"))
    }

    pub fn prepare(&self, cancel: &AtomicBool) -> Result<Runtime, String> {
        check_cancel(cancel)?;
        if !self.root.is_absolute() {
            return Err("The GPU runtime directory must be absolute.".into());
        }
        if let Some(parent) = self.root.parent() {
            normal_directory(parent)?;
        }
        let detected = device::discover()?;
        let backend = match detected.backend.as_str() {
            "trt" => Backend::TensorRt,
            "openvino" => Backend::OpenVino,
            _ => return Err("Unsupported native GPU runtime backend.".into()),
        };
        let selected = assets::selected(backend)?;
        let mut identity = Sha256::new();
        identity.update(RUNTIME_ABI);
        for asset in &selected {
            identity.update(asset.sha256);
        }
        let runtime_id = format!("native-{:x}", identity.finalize());
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Create GPU runtime directory: {error}"))?;
        normal_directory(&self.root)?;
        let _installer = ExclusiveFileLock::wait(&self.root.join("native-install.lock"), cancel)?;
        for name in ["cache", "versions", "engines", "staging"] {
            let directory = self.root.join(name);
            fs::create_dir_all(&directory)
                .map_err(|error| format!("Create GPU runtime layout: {error}"))?;
            normal_directory(&directory)?;
        }
        if let Some(runtime) = self.inspect(&detected, &runtime_id, cancel)? {
            return Ok(runtime);
        }
        let cache = self.root.join("cache");
        let mut required = 512_u64 * 1024 * 1024;
        for asset in &selected {
            required = required.saturating_add(asset.installed_bytes);
            if let assets::AssetSource::Network { bytes, .. } = asset.source
                && !crate::files::verified_file(&cache.join(asset.filename), bytes, asset.sha256)?
            {
                required = required.saturating_add(bytes);
            }
        }
        if storage::available_bytes(&self.root)? < required {
            return Err("framegenDiskSpace".into());
        }
        let stage = Staging::directory(&self.root.join("staging"))?;
        for asset in selected {
            check_cancel(cancel)?;
            let source = archive::obtain(asset, &cache, &self.engine_root, cancel)?;
            extract_component(asset, &source, &stage.path, &detected.capability, cancel)?;
        }
        notices::write(&stage.path, backend)
            .map_err(|error| format!("Install GPU notices: {error}"))?;
        let mut device_binding = detected;
        let openvino_device = if backend == Backend::OpenVino {
            device::resolve_openvino(
                &stage.path.join("plugins/vsov/openvino_c.dll"),
                &mut device_binding,
            )?
        } else {
            String::new()
        };
        let files = collect_files(&stage.path, cancel)?;
        if required_paths(backend, &device_binding.capability)
            .iter()
            .any(|name| !files.contains_key(name))
        {
            return Err("The native GPU runtime archive omitted a required file.".into());
        }
        let mut digest = Sha256::new();
        digest.update(&runtime_id);
        digest.update(serde_json::to_vec(&device_binding).map_err(|error| error.to_string())?);
        digest.update(serde_json::to_vec(&files).map_err(|error| error.to_string())?);
        let engine_dir = self
            .root
            .join("engines")
            .join(format!("{:x}", digest.finalize()));
        fs::create_dir_all(&engine_dir)
            .map_err(|error| format!("Create native engine cache: {error}"))?;
        normal_directory(&engine_dir)?;
        let version_dir = self
            .root
            .join("versions")
            .join(stage.path.file_name().ok_or("GPU staging has no name.")?);
        check_cancel(cancel)?;
        crate::files::move_new_file(&stage.path, &version_dir)
            .map_err(|error| format!("Publish native GPU files: {error}"))?;
        let publication = (|| {
            let runtime = Runtime {
                runtime_id,
                version_dir: version_dir.clone(),
                engine_dir,
                gpu_lock_path: self.gpu_lock_path()?,
                device_binding,
                openvino_device,
                files,
                prepared_engine: None,
            };
            let manifest_stage = Staging::directory(&self.root.join("staging"))?;
            let manifest = manifest_stage.path.join(MANIFEST);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&manifest)
                .map_err(|error| error.to_string())?;
            serde_json::to_writer_pretty(&mut output, &runtime)
                .map_err(|error| error.to_string())?;
            output.sync_all().map_err(|error| error.to_string())?;
            drop(output);
            check_cancel(cancel)?;
            storage::replace_manifest(&manifest, &self.root.join(MANIFEST))?;
            Ok(runtime)
        })();
        if publication.is_err() {
            // This generation has not become visible through the manifest.
            // Remove only its newly published directory, preserving the last
            // accepted runtime and every engine belonging to it.
            fs::remove_dir_all(&version_dir)
                .map_err(|error| format!("Remove unpublished GPU runtime: {error}"))?;
        }
        publication
    }
}

fn extract_component(
    asset: Asset,
    source: &Path,
    root: &Path,
    capability: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if asset.id == "vapoursynth" {
        archive::extract(asset.kind, source, root, cancel, |name| {
            (name == "wheel/vapoursynth-79-cp312-abi3-win_amd64.whl")
                .then(|| PathBuf::from("vapoursynth.whl"))
        })?;
        let wheel = root.join("vapoursynth.whl");
        archive::extract(ArchiveKind::Zip, &wheel, root, cancel, |name| {
            [
                "vapoursynth/libvapoursynth.dll",
                "vapoursynth/libvapoursynthfilters.dll",
                "vapoursynth/libvapoursynthfilters_avx2.dll",
                "vapoursynth/libvapoursynthfilters_zn4.dll",
                "vapoursynth-79.dist-info/licenses/COPYING.LESSER",
            ]
            .contains(&name)
            .then(|| PathBuf::from(name))
        })?;
        fs::remove_file(wheel).map_err(|error| error.to_string())?;
        return Ok(());
    }
    archive::extract(asset.kind, source, root, cancel, |name| {
        let target = match asset.id {
            "bundled-native" => match asset.source {
                assets::AssetSource::Bundled { relative_path, .. } => relative_path.to_owned(),
                _ => return None,
            },
            "vstrt" if name == "vstrt.dll" => "plugins/vstrt.dll".to_owned(),
            "vsov" if name == asset.filename => "plugins/vsov.dll".to_owned(),
            "openvino-runtime" => {
                let filename = name.strip_prefix("openvino/libs/")?;
                if !OPENVINO_FILES.contains(&filename) {
                    return None;
                }
                format!("plugins/vsov/{filename}")
            }
            "tensorrt-libs" => {
                let filename = name.rsplit('/').next()?;
                if name.ends_with(".dist-info/LICENSE.txt") {
                    "notices/TensorRT-LICENSE.txt".into()
                } else if TRT_FILES.contains(&filename)
                    || assets::tensorrt_resources(capability)
                        .iter()
                        .any(|name| name == filename)
                {
                    format!("{TRT_DIRECTORY}/{filename}")
                } else {
                    return None;
                }
            }
            "rife-4.26-trt" | "rife-4.26-openvino-s0p5"
                if name == "rife_v4.26.onnx" || name == "rife_v2/rife_v4.26.onnx" =>
            {
                "models/rife_v2/rife_v4.26.onnx".into()
            }
            _ => return None,
        };
        Some(PathBuf::from(target))
    })
}

const TRT_DIRECTORY: &str = if cfg!(windows) {
    "plugins/vsmlrt-cuda"
} else {
    "lib"
};

#[cfg(target_os = "linux")]
const TRT_FILES: &[&str] = &[
    "libnvinfer.so.10",
    "libnvinfer_plugin.so.10",
    "libnvonnxparser.so.10",
];
#[cfg(windows)]
const TRT_FILES: &[&str] = &[
    "nvinfer_10.dll",
    "nvinfer_plugin_10.dll",
    "nvonnxparser_10.dll",
];
const OPENVINO_FILES: &[&str] = &[
    "cache.json",
    "openvino.dll",
    "openvino_c.dll",
    "openvino_intel_gpu_plugin.dll",
    "openvino_onnx_frontend.dll",
    "tbb12.dll",
];

fn required_paths(backend: Backend, capability: &str) -> Vec<String> {
    #[cfg(windows)]
    let mut paths = vec![
        "vapoursynth/libvapoursynth.dll".into(),
        "vapoursynth/libvapoursynthfilters.dll".into(),
        "models/rife_v2/rife_v4.26.onnx".into(),
    ];
    #[cfg(target_os = "linux")]
    let mut paths: Vec<String> = assets::LINUX_NATIVE_FILES
        .iter()
        .map(|asset| match asset.source {
            assets::AssetSource::Bundled { relative_path, .. } => relative_path.to_owned(),
            _ => unreachable!("bundled native asset inventory"),
        })
        .chain(std::iter::once("models/rife_v2/rife_v4.26.onnx".into()))
        .collect();
    match backend {
        Backend::TensorRt => {
            #[cfg(windows)]
            paths.push("plugins/vstrt.dll".into());
            paths.extend(
                TRT_FILES
                    .iter()
                    .map(|name| format!("{TRT_DIRECTORY}/{name}")),
            );
            paths.extend(
                assets::tensorrt_resources(capability)
                    .iter()
                    .map(|name| format!("{TRT_DIRECTORY}/{name}")),
            );
        }
        Backend::OpenVino => {
            paths.push("plugins/vsov.dll".into());
            paths.extend(
                OPENVINO_FILES
                    .iter()
                    .map(|name| format!("plugins/vsov/{name}")),
            );
        }
    }
    paths
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("GPU preparation was cancelled.".into())
    } else {
        Ok(())
    }
}
