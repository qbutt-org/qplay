use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::yt_dlp::YtDlp;

pub(crate) const VERSION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../qplay-version.txt"
));

#[derive(Deserialize)]
struct BuildManifest {
    product: String,
    version: String,
    files: BTreeMap<String, String>,
}

pub fn version_json() -> String {
    json!({
        "product": "qplay",
        "version": VERSION.trim(),
        "protocolVersion": 1,
    })
    .to_string()
}

pub fn runtime_info_json(library: &str, data: &str, config: &str) -> Result<String, String> {
    let library = Path::new(library);
    let engine = library
        .parent()
        .ok_or("The mpv library needs an engine directory.")?;
    let bundle = engine
        .parent()
        .ok_or("The mpv engine needs an application directory.")?;
    verify_bundle(bundle)?;
    let data = Path::new(data);
    let tools = YtDlp::new(library, data)?;
    let yt_dlp = tools
        .select()?
        .ok_or("The bundled yt-dlp executable is missing or invalid.")?;

    let mpv = tool(engine, "mpv.exe")?;
    let ffmpeg = tool(engine, "ffmpeg.exe")?;
    let ffprobe = tool(engine, "ffprobe.exe")?;
    let deno = tools.deno_path()?;
    let metadata = json!({
        "product": "qplay",
        "version": VERSION.trim(),
        "protocolVersion": 1,
        "runtimeDir": engine,
        "configDir": config,
        "dataDir": data,
        "tools": {
            "mpv": mpv,
            "ffmpeg": ffmpeg,
            "ffprobe": ffprobe,
            "ytDlp": yt_dlp.path,
        },
        "onlineTools": {
            "ytDlpVersion": yt_dlp.version,
            "deno": deno.map(|path| json!({ "path": path, "version": tools.deno_version() })),
        },
    });
    Ok(metadata.to_string())
}

fn verify_bundle(bundle: &Path) -> Result<(), String> {
    let bytes = fs::read(bundle.join("build-manifest.json"))
        .map_err(|error| format!("Read qplay build manifest: {error}"))?;
    let manifest: BuildManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Parse qplay build manifest: {error}"))?;
    if manifest.product != "qplay" || manifest.version != VERSION.trim() {
        return Err("The qplay build manifest does not match this player.".into());
    }
    for required in [
        "qplay.exe",
        "engine/libmpv-2.dll",
        "engine/mpv.exe",
        "engine/ffmpeg.exe",
        "engine/ffprobe.exe",
        "engine/yt-dlp.exe",
    ] {
        if !manifest.files.contains_key(required) {
            return Err(format!("The qplay build manifest omits {required}."));
        }
    }

    let bundle =
        fs::canonicalize(bundle).map_err(|error| format!("Resolve qplay bundle: {error}"))?;
    for (name, expected) in manifest.files {
        if name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
            || name.contains('\\')
            || name.contains(':')
            || expected.len() != 64
            || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("Invalid qplay build manifest entry: {name}"));
        }

        let path = bundle.join(&name);
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| format!("Read bundled {name}: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!("Bundled {name} is missing or invalid."));
        }
        let resolved =
            fs::canonicalize(&path).map_err(|error| format!("Resolve bundled {name}: {error}"))?;
        if !resolved.starts_with(&bundle) {
            return Err(format!("Bundled {name} escapes the application directory."));
        }

        let mut file =
            File::open(&path).map_err(|error| format!("Open bundled {name}: {error}"))?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("Read bundled {name}: {error}"))?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        if format!("{:x}", digest.finalize()) != expected {
            return Err(format!("Bundled {name} differs from the build manifest."));
        }
    }
    Ok(())
}

fn tool(engine: &Path, name: &str) -> Result<PathBuf, String> {
    let path = engine.join(name);
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("Read bundled {name}: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("Bundled {name} is missing or invalid."));
    }
    Ok(path)
}
