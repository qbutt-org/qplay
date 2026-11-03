use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::assets::{ArchiveKind, Asset, AssetSource};

pub fn obtain(
    asset: Asset,
    cache: &Path,
    engine_root: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let (path, bytes) = match asset.source {
        AssetSource::Bundled {
            relative_path,
            bytes,
        } => (engine_root.join(relative_path), bytes),
        AssetSource::Network { bytes, .. } => (cache.join(asset.filename), bytes),
    };
    if crate::files::verified_file(&path, bytes, asset.sha256)? {
        return Ok(path);
    }
    let AssetSource::Network { url, .. } = asset.source else {
        return Err(format!("The bundled {} is damaged.", asset.label));
    };
    if path.exists() {
        super::storage::normal_file(&path)?;
        fs::remove_file(&path).map_err(|error| format!("Remove damaged GPU download: {error}"))?;
    }
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent("qplay native frame generation")
        .build()
        .map_err(|error| format!("Prepare GPU download: {error}"))?;
    let partial = cache.join(format!("{}.partial", asset.filename));
    if partial.exists() {
        super::storage::normal_file(&partial)?;
        fs::remove_file(&partial).map_err(|error| format!("Remove stale GPU download: {error}"))?;
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|error| format!("Stage GPU download: {error}"))?;
    let result = crate::files::run_network(
        cancel,
        crate::files::download_file(
            &client,
            url,
            bytes,
            asset.sha256,
            &mut output,
            &mut |_, _| {},
            asset.label,
        ),
    );
    drop(output);
    let result = match result {
        Ok(Some(())) => crate::files::move_new_file(&partial, &path)
            .map_err(|error| format!("Publish GPU download: {error}")),
        Ok(None) => Err("GPU preparation was cancelled.".into()),
        Err(error) => Err(error),
    };
    if partial.is_file() {
        fs::remove_file(&partial)
            .map_err(|error| format!("Remove partial GPU download: {error}"))?;
    }
    result?;
    Ok(path)
}

fn relative_path(name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(name);
    if name.is_empty()
        || name.contains([':', '\0', '\\'])
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("The GPU archive contains an unsafe path.".into());
    }
    Ok(path)
}

fn copy_entry(reader: &mut dyn Read, path: &Path, cancel: &AtomicBool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::other("GPU preparation was cancelled."));
        }
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return output.sync_all();
        }
        output.write_all(&buffer[..count])?;
    }
}

/// Extract only native files into a new private staging directory. Archive
/// paths and mapped targets are both checked; skipped solid 7z entries drain.
pub fn extract(
    kind: ArchiveKind,
    source: &Path,
    destination: &Path,
    cancel: &AtomicBool,
    map: impl Fn(&str) -> Option<PathBuf>,
) -> Result<(), String> {
    match kind {
        ArchiveKind::File => {
            let name = source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Invalid GPU asset filename.")?;
            let target = map(name).ok_or("The bundled GPU asset has no installation target.")?;
            relative_path(target.to_str().ok_or("Invalid GPU destination path.")?)?;
            copy_entry(
                &mut File::open(source).map_err(|error| error.to_string())?,
                &destination.join(target),
                cancel,
            )
            .map_err(|error| error.to_string())
        }
        ArchiveKind::Zip => {
            let mut zip =
                zip::ZipArchive::new(File::open(source).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            for index in 0..zip.len() {
                if cancel.load(Ordering::Acquire) {
                    return Err("GPU preparation was cancelled.".into());
                }
                let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
                let name = entry.name().replace('\\', "/");
                relative_path(name.trim_end_matches('/'))?;
                let Some(target) = map(&name) else { continue };
                relative_path(target.to_str().ok_or("Invalid GPU destination path.")?)?;
                if entry.is_file() {
                    copy_entry(&mut entry, &destination.join(target), cancel)
                        .map_err(|error| error.to_string())?;
                }
            }
            Ok(())
        }
        ArchiveKind::SevenZip => sevenz_rust2::decompress_file_with_extract_fn(
            source,
            destination,
            |entry, reader, _| {
                let name = entry.name().replace('\\', "/");
                relative_path(name.trim_end_matches('/')).map_err(io::Error::other)?;
                if let Some(target) = map(&name) {
                    relative_path(
                        target
                            .to_str()
                            .ok_or_else(|| io::Error::other("Invalid GPU destination path."))?,
                    )
                    .map_err(io::Error::other)?;
                    if !entry.is_directory() {
                        copy_entry(reader, &destination.join(target), cancel)?;
                    }
                } else if !entry.is_directory() {
                    let mut buffer = [0_u8; 64 * 1024];
                    while reader.read(&mut buffer)? != 0 {
                        if cancel.load(Ordering::Acquire) {
                            return Err(io::Error::other("GPU preparation was cancelled.").into());
                        }
                    }
                }
                Ok(true)
            },
        )
        .map_err(|error| error.to_string()),
    }
}
