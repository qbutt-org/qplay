use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::yt_dlp::YtDlp;

const VERSION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../qplay-version.txt"
));

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

fn tool(engine: &Path, name: &str) -> Result<PathBuf, String> {
    let path = engine.join(name);
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("Read bundled {name}: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("Bundled {name} is missing or invalid."));
    }
    Ok(path)
}
