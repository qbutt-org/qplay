use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::files::move_new_file;

const RELEASE_URL: &str =
    "https://api.github.com/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest";
const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/download/";
const MAX_RELEASE_JSON: u64 = 1024 * 1024;
const MAX_EXECUTABLE_SIZE: u64 = 128 * 1024 * 1024;
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 60);
static NEXT_STAGE: AtomicU64 = AtomicU64::new(1);

pub struct YtDlp {
    bundled: PathBuf,
    updates: PathBuf,
    bundled_version: String,
    bundled_hash: String,
    deno: DenoPin,
    stage_id: u64,
}

pub struct Executable {
    pub path: PathBuf,
    pub version: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DenoPin {
    version: String,
    url: String,
    sha256: String,
    size: u64,
    executable_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildLock {
    yt_dlp: BundledVersion,
    deno: DenoPin,
    runtime: RuntimeFiles,
}

#[derive(Deserialize)]
struct BundledVersion {
    version: String,
}

#[derive(Deserialize)]
struct RuntimeFiles {
    files: HashMap<String, String>,
}

impl YtDlp {
    pub fn new(library: &Path, data: &Path) -> Result<Self, String> {
        let lock: BuildLock = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../upstream-lock.json"
        )))
        .map_err(|error| format!("Read pinned online tools: {error}"))?;
        let bundled_hash = lock
            .runtime
            .files
            .get("tools/yt-dlp.exe")
            .ok_or("Pinned yt-dlp digest is missing.")?;
        if parse_version(&lock.yt_dlp.version).is_none()
            || !valid_digest(bundled_hash)
            || !valid_digest(&lock.deno.sha256)
            || !valid_digest(&lock.deno.executable_sha256)
            || lock.deno.size == 0
            || lock.deno.size > MAX_EXECUTABLE_SIZE
            || lock.deno.url
                != format!(
                    "https://github.com/denoland/deno/releases/download/v{}/deno-x86_64-pc-windows-msvc.zip",
                    lock.deno.version
                )
        {
            return Err("Pinned online tool metadata is invalid.".into());
        }
        let engine = library
            .parent()
            .ok_or("The mpv library needs an engine directory.")?;
        Ok(Self {
            bundled: engine.join("yt-dlp.exe"),
            updates: data
                .join("yt-dlp")
                .join(format!("deno-{}", lock.deno.version)),
            bundled_version: lock.yt_dlp.version,
            bundled_hash: bundled_hash.to_owned(),
            deno: lock.deno,
            stage_id: NEXT_STAGE.fetch_add(1, Ordering::Relaxed),
        })
    }

    pub fn select(&self) -> Result<Option<Executable>, String> {
        let mut selected = if verified_file(&self.bundled, &self.bundled_hash, None)? {
            Some(Executable {
                path: self.bundled.clone(),
                version: self.bundled_version.clone(),
            })
        } else {
            None
        };

        let entries = match fs::read_dir(&self.updates) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(selected),
            Err(error) => return Err(format!("Read yt-dlp updates: {error}")),
        };
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("Read yt-dlp update entry: {error}"))?;
            if !entry
                .file_type()
                .map_err(|error| format!("Inspect yt-dlp update entry: {error}"))?
                .is_file()
            {
                continue;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some((version, release_tag, digest)) = parse_filename(name) else {
                continue;
            };
            candidates.push((version, release_tag.to_owned(), digest.to_owned(), path));
        }
        candidates.sort_unstable_by(|left, right| right.0.cmp(&left.0));

        for (version, release_tag, digest, path) in candidates {
            if selected.as_ref().is_some_and(|current| {
                version < parse_version(&current.version).unwrap_or((0, 0, 0, 0))
            }) {
                break;
            }
            if verified_file(&path, &digest, None)? {
                selected = Some(Executable {
                    path,
                    version: release_tag,
                });
                break;
            }
        }
        Ok(selected)
    }

    pub fn deno_path(&self) -> Result<Option<PathBuf>, String> {
        let path = self.updates.join("deno.exe");
        Ok(verified_file(&path, &self.deno.executable_sha256, None)?.then_some(path))
    }

    pub fn deno_version(&self) -> &str {
        &self.deno.version
    }

    pub fn ensure_youtube(
        &self,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Option<Executable>, String> {
        if !self.ensure_deno(cancel, &mut progress)? {
            return Ok(None);
        }
        let selected = match self.select()? {
            Some(selected) => selected,
            None => match self.fetch_update(cancel, &mut progress)? {
                Some(downloaded) => downloaded,
                None => return Ok(None),
            },
        };
        if selected.path != self.bundled {
            return Ok(Some(selected));
        }

        let target = self.updates.join(format!(
            "yt-dlp-{}-{}.exe",
            self.bundled_version, self.bundled_hash
        ));
        if verified_file(&target, &self.bundled_hash, None)? {
            return Ok(Some(Executable {
                path: target,
                version: self.bundled_version.clone(),
            }));
        }
        if fs::symlink_metadata(&target).is_ok() {
            fs::remove_file(&target)
                .map_err(|error| format!("Remove invalid yt-dlp copy: {error}"))?;
        }
        let partial = self.updates.join(format!(
            "yt-dlp-copy-{}-{}.partial",
            std::process::id(),
            self.stage_id
        ));
        remove_stale_partial(&partial)?;
        let result = (|| {
            let mut source = File::open(&self.bundled)
                .map_err(|error| format!("Read bundled yt-dlp: {error}"))?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)
                .map_err(|error| format!("Stage bundled yt-dlp: {error}"))?;
            if !copy_with_cancel(&mut source, &mut output, cancel)? {
                return Ok(None);
            }
            output
                .sync_all()
                .map_err(|error| format!("Flush bundled yt-dlp: {error}"))?;
            drop(output);
            if cancel.load(Ordering::Acquire) {
                return Ok(None);
            }
            if !verified_file(&partial, &self.bundled_hash, None)? {
                return Err("Bundled yt-dlp copy failed integrity verification.".into());
            }
            if let Err(error) = move_new_file(&partial, &target) {
                if !verified_file(&target, &self.bundled_hash, None)? {
                    return Err(format!("Install bundled yt-dlp copy: {error}"));
                }
            }
            Ok(Some(Executable {
                path: target,
                version: self.bundled_version.clone(),
            }))
        })();
        let _ = fs::remove_file(&partial);
        result
    }

    fn ensure_deno(
        &self,
        cancel: &AtomicBool,
        progress: &mut impl FnMut(u64, u64),
    ) -> Result<bool, String> {
        let target = self.updates.join("deno.exe");
        if verified_file(&target, &self.deno.executable_sha256, None)? {
            return Ok(true);
        }
        if fs::symlink_metadata(&target).is_ok() {
            fs::remove_file(&target)
                .map_err(|error| format!("Remove invalid Deno executable: {error}"))?;
        }
        if cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        fs::create_dir_all(&self.updates)
            .map_err(|error| format!("Create online tools directory: {error}"))?;
        let archive_path = self.updates.join(format!(
            "deno-{}-{}.partial",
            std::process::id(),
            self.stage_id
        ));
        let executable_path = self.updates.join(format!(
            "deno-exe-{}-{}.partial",
            std::process::id(),
            self.stage_id
        ));
        remove_stale_partial(&archive_path)?;
        remove_stale_partial(&executable_path)?;

        let result = (|| {
            let client = http_client()?;
            if !download_file(
                &client,
                &self.deno.url,
                self.deno.size,
                &self.deno.sha256,
                &archive_path,
                cancel,
                progress,
                "Deno",
            )? {
                return Ok(false);
            }

            let archive =
                File::open(&archive_path).map_err(|error| format!("Open Deno archive: {error}"))?;
            let mut archive =
                ZipArchive::new(archive).map_err(|error| format!("Read Deno archive: {error}"))?;
            if archive.len() != 1 {
                return Err("Deno archive has unexpected contents.".into());
            }
            let mut deno = archive
                .by_name("deno.exe")
                .map_err(|error| format!("Find Deno executable: {error}"))?;
            if deno.size() > MAX_EXECUTABLE_SIZE {
                return Err("Deno executable exceeds the size limit.".into());
            }
            let mut executable = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&executable_path)
                .map_err(|error| format!("Stage Deno executable: {error}"))?;
            if !copy_with_cancel(&mut deno, &mut executable, cancel)? {
                return Ok(false);
            }
            executable
                .sync_all()
                .map_err(|error| format!("Flush Deno executable: {error}"))?;
            drop(executable);
            if cancel.load(Ordering::Acquire) {
                return Ok(false);
            }
            if !verified_file(&executable_path, &self.deno.executable_sha256, None)? {
                return Err("Deno executable failed integrity verification.".into());
            }
            if let Err(error) = move_new_file(&executable_path, &target) {
                if !verified_file(&target, &self.deno.executable_sha256, None)? {
                    return Err(format!("Install Deno: {error}"));
                }
            }
            Ok(true)
        })();
        let _ = fs::remove_file(&archive_path);
        let _ = fs::remove_file(&executable_path);
        result
    }

    pub fn update_if_due(
        &self,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Option<Executable>, String> {
        if cancel.load(Ordering::Acquire) || !self.check_due()? {
            return Ok(None);
        }
        fs::create_dir_all(&self.updates)
            .map_err(|error| format!("Create yt-dlp update directory: {error}"))?;
        self.mark_check(false)?;

        let result = self.fetch_update(cancel, &mut progress);
        if result.is_ok() && !cancel.load(Ordering::Acquire) {
            self.mark_check(true)?;
        }
        result
    }

    fn check_due(&self) -> Result<bool, String> {
        let marker = self.updates.join("last-check");
        let metadata = match fs::metadata(&marker) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(format!("Read yt-dlp check time: {error}")),
        };
        let age = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .unwrap_or(Duration::MAX);
        let interval = if fs::read(&marker).ok().as_deref() == Some(b"ok") {
            CHECK_INTERVAL
        } else {
            RETRY_INTERVAL
        };
        Ok(age >= interval)
    }

    fn mark_check(&self, success: bool) -> Result<(), String> {
        fs::write(
            self.updates.join("last-check"),
            if success { "ok" } else { "retry" },
        )
        .map_err(|error| format!("Record yt-dlp check time: {error}"))
    }

    fn fetch_update(
        &self,
        cancel: &AtomicBool,
        progress: &mut impl FnMut(u64, u64),
    ) -> Result<Option<Executable>, String> {
        let client = http_client()?;
        let response = client
            .get(RELEASE_URL)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| format!("Check yt-dlp release: {error}"))?;
        let mut json = Vec::new();
        response
            .take(MAX_RELEASE_JSON + 1)
            .read_to_end(&mut json)
            .map_err(|error| format!("Read yt-dlp release: {error}"))?;
        if json.len() as u64 > MAX_RELEASE_JSON {
            return Err("yt-dlp release metadata is too large.".into());
        }
        let release: Release = serde_json::from_slice(&json)
            .map_err(|error| format!("Parse yt-dlp release: {error}"))?;
        let version =
            parse_version(&release.tag_name).ok_or("yt-dlp release has an unexpected version.")?;
        if self
            .select()?
            .and_then(|current| parse_version(&current.version))
            .is_some_and(|current| current >= version)
        {
            return Ok(None);
        }
        let asset = release
            .assets
            .into_iter()
            .find(|asset| asset.name == "yt-dlp.exe")
            .ok_or("yt-dlp release has no Windows executable.")?;
        let expected_url = format!("{}{}/yt-dlp.exe", RELEASE_DOWNLOAD_PREFIX, release.tag_name);
        if asset.browser_download_url != expected_url
            || asset.size == 0
            || asset.size > MAX_EXECUTABLE_SIZE
        {
            return Err("yt-dlp release asset metadata is invalid.".into());
        }
        let digest = asset
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
            .filter(|value| valid_digest(value))
            .ok_or("yt-dlp release asset has no SHA-256 digest.")?;
        let filename = format!("yt-dlp-{}-{}.exe", release.tag_name, digest);
        let target = self.updates.join(filename);
        if verified_file(&target, digest, Some(asset.size))? {
            return Ok(Some(Executable {
                path: target,
                version: release.tag_name,
            }));
        }
        if fs::symlink_metadata(&target).is_ok() {
            fs::remove_file(&target)
                .map_err(|error| format!("Remove invalid yt-dlp update: {error}"))?;
        }
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }

        let partial = self.updates.join(format!(
            "yt-dlp-{}-{}-{}.partial",
            release.tag_name,
            std::process::id(),
            self.stage_id
        ));
        remove_stale_partial(&partial)?;
        let download = (|| {
            if !download_file(
                &client,
                &asset.browser_download_url,
                asset.size,
                digest,
                &partial,
                cancel,
                progress,
                "yt-dlp",
            )? {
                return Ok(None);
            }
            if let Err(error) = move_new_file(&partial, &target) {
                if !verified_file(&target, digest, Some(asset.size))? {
                    return Err(format!("Install yt-dlp update: {error}"));
                }
            }
            Ok(Some(Executable {
                path: target,
                version: release.tag_name,
            }))
        })();
        if partial.exists() {
            let _ = fs::remove_file(&partial);
        }
        download
    }
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .user_agent("qplay online media tools")
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Create online tools HTTP client: {error}"))
}

fn download_file(
    client: &Client,
    url: &str,
    expected_size: u64,
    expected_digest: &str,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, u64),
    label: &str,
) -> Result<bool, String> {
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Download {label}: {error}"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("Stage {label}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut written = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        let count = response
            .read(&mut buffer)
            .map_err(|error| format!("Read {label} download: {error}"))?;
        if count == 0 {
            break;
        }
        written += count as u64;
        if written > expected_size {
            return Err(format!("{label} download exceeds its expected size."));
        }
        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("Stage {label}: {error}"))?;
        progress(written, expected_size);
    }
    if written != expected_size || format!("{:x}", hasher.finalize()) != expected_digest {
        return Err(format!("{label} download failed integrity verification."));
    }
    output
        .sync_all()
        .map_err(|error| format!("Flush {label} download: {error}"))?;
    Ok(!cancel.load(Ordering::Acquire))
}

fn copy_with_cancel(
    input: &mut impl Read,
    output: &mut impl Write,
    cancel: &AtomicBool,
) -> Result<bool, String> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        let count = input
            .read(&mut buffer)
            .map_err(|error| format!("Read online tool: {error}"))?;
        if count == 0 {
            return Ok(true);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("Stage online tool: {error}"))?;
    }
}

fn remove_stale_partial(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Remove incomplete online tool: {error}")),
    }
}

fn verified_file(path: &Path, digest: &str, size: Option<u64>) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Inspect online tool: {error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Read online tool: {error}")),
    };
    let length = file
        .metadata()
        .map_err(|error| format!("Inspect online tool: {error}"))?
        .len();
    if length > MAX_EXECUTABLE_SIZE || size.is_some_and(|expected| length != expected) {
        return Ok(false);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Hash online tool: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()) == digest)
}

fn parse_filename(name: &str) -> Option<((u32, u32, u32, u32), &str, &str)> {
    let name = name.strip_prefix("yt-dlp-")?.strip_suffix(".exe")?;
    let (version, digest) = name.split_once('-')?;
    if !valid_digest(digest) {
        return None;
    }
    Some((parse_version(version)?, version, digest))
}

fn parse_version(version: &str) -> Option<(u32, u32, u32, u32)> {
    let mut parts = version.split('.');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    let time = match parts.next() {
        Some(value) if value.len() == 6 => value.parse().ok()?,
        None => 0,
        _ => return None,
    };
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day, time))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn is_youtube_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com")
}
