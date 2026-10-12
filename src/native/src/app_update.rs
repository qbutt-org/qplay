use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use reqwest::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows::Win32::System::JobObjects::IsProcessInJob;
use windows::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, GetCurrentProcess};

use crate::files::move_new_file;
use crate::runtime_info::VERSION;

const RELEASE_URL: &str = "https://api.github.com/repos/qbutt-org/qplay/releases/latest";
const DOWNLOAD_PREFIX: &str = "https://github.com/qbutt-org/qplay/releases/download/";
const MAX_RELEASE_JSON: u64 = 1024 * 1024;
const MAX_INSTALLER_SIZE: u64 = 1024 * 1024 * 1024;
static NEXT_STAGE: AtomicU64 = AtomicU64::new(1);

pub enum CheckResult {
    Unpublished,
    Current,
    Available(Release),
}

#[derive(Clone)]
pub struct Release {
    pub version: String,
    setup: VerifiedAsset,
    pub portable_url: String,
}

#[derive(Deserialize)]
struct AssetMetadata {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone)]
struct VerifiedAsset {
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<AssetMetadata>,
}

pub fn check(cancel: &AtomicBool) -> Result<Option<CheckResult>, String> {
    run_network(cancel, async {
        let client = client(Duration::from_secs(30))?;
        let mut response = client
            .get(RELEASE_URL)
            .send()
            .await
            .map_err(|error| format!("Check qplay release: {error}"))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(CheckResult::Unpublished);
        }
        response = response
            .error_for_status()
            .map_err(|error| format!("Check qplay release: {error}"))?;
        let mut json = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("Read qplay release: {error}"))?
        {
            if json.len() as u64 + chunk.len() as u64 > MAX_RELEASE_JSON {
                return Err("qplay release metadata is too large.".into());
            }
            json.extend_from_slice(&chunk);
        }

        let release: GithubRelease = serde_json::from_slice(&json)
            .map_err(|error| format!("Parse qplay release: {error}"))?;
        let version = release
            .tag_name
            .strip_prefix('v')
            .and_then(parse_version)
            .ok_or("qplay release has an invalid version number.")?;
        if version <= parse_version(VERSION.trim()).expect("pinned qplay version") {
            return Ok(CheckResult::Current);
        }

        let setup = select_asset(&release, "qplay-setup.exe")?;
        let portable = select_asset(&release, "qplay-portable.zip")?;
        Ok(CheckResult::Available(Release {
            version: release.tag_name[1..].into(),
            setup,
            portable_url: portable.url,
        }))
    })
}

fn select_asset(release: &GithubRelease, name: &str) -> Result<VerifiedAsset, String> {
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches
        .next()
        .ok_or("qplay release is missing its Windows assets.")?;
    if matches.next().is_some()
        || asset.browser_download_url != format!("{}{}/{}", DOWNLOAD_PREFIX, release.tag_name, name)
        || asset.size == 0
        || asset.size > MAX_INSTALLER_SIZE
    {
        return Err("qplay release has invalid Windows asset metadata.".into());
    }
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(valid_digest)
        .ok_or("qplay release asset has no valid SHA-256 digest.")?;
    Ok(VerifiedAsset {
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sha256: sha256.into(),
    })
}

pub fn download(
    release: &Release,
    data: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(i32),
) -> Result<Option<PathBuf>, String> {
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let digest = release.setup.sha256.as_str();
    let directory = data.join("updates");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Create qplay update directory: {error}"))?;
    let cached = directory.join(format!("qplay-v{}-{digest}-setup.exe", release.version));
    if verified_file(&cached, release.setup.size, digest)? {
        return Ok(Some(cached));
    }
    let attempt = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
    let target = if fs::symlink_metadata(&cached).is_ok() {
        directory.join(format!(
            "qplay-v{}-{digest}-{}-{attempt}-setup.exe",
            release.version,
            std::process::id()
        ))
    } else {
        cached
    };
    let partial = directory.join(format!(
        "qplay-v{}-{}-{}.partial",
        release.version,
        std::process::id(),
        attempt
    ));
    let mut owns_partial = false;
    let result = run_network(cancel, async {
        let client = client(Duration::from_secs(600))?;
        let mut response = client
            .get(&release.setup.url)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|error| format!("Download qplay update: {error}"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|error| format!("Stage qplay update: {error}"))?;
        owns_partial = true;
        let mut hash = Sha256::new();
        let mut received = 0_u64;

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("Read qplay update: {error}"))?
        {
            received += chunk.len() as u64;
            if received > release.setup.size {
                return Err("qplay update exceeds its published size.".into());
            }
            hash.update(&chunk);
            file.write_all(&chunk)
                .map_err(|error| format!("Stage qplay update: {error}"))?;
            progress((received * 100 / release.setup.size) as i32);
        }
        if received != release.setup.size || format!("{:x}", hash.finalize()) != digest {
            return Err("qplay update failed its integrity check.".into());
        }
        file.sync_all()
            .map_err(|error| format!("Flush qplay update: {error}"))?;
        drop(file);
        if cancel.load(Ordering::Acquire) {
            return Err("qplay update cancelled.".into());
        }
        if let Err(error) = move_new_file(&partial, &target) {
            if !verified_file(&target, release.setup.size, digest)? {
                return Err(format!("Finish qplay update: {error}"));
            }
        }
        Ok(target)
    });
    if owns_partial {
        let _ = fs::remove_file(&partial);
    }
    result
}

pub fn start_installer(
    release: &Release,
    path: &Path,
    install_directory: &Path,
) -> Result<(), String> {
    let digest = release.setup.sha256.as_str();
    if !verified_file(path, release.setup.size, digest)? {
        return Err("The staged qplay update failed integrity verification.".into());
    }
    let mut in_job = windows::core::BOOL::default();
    unsafe { IsProcessInJob(GetCurrentProcess(), None, &mut in_job) }
        .map_err(|error| format!("Inspect qplay process job: {error}"))?;

    let mut command = Command::new(path);
    command
        .arg("/CURRENTUSER")
        .arg("/VERYSILENT")
        .arg("/SUPPRESSMSGBOXES")
        .arg("/NORESTART")
        .arg("/SP-")
        .arg(format!("/DIR={}", install_directory.display()))
        .arg(format!("/QPLAYUPDATE={}", std::process::id()))
        .current_dir(install_directory);
    if in_job.as_bool() {
        command.creation_flags(CREATE_BREAKAWAY_FROM_JOB.0);
    }
    command
        .spawn()
        .map_err(|error| format!("Start qplay installer: {error}"))?;
    Ok(())
}

fn verified_file(path: &Path, size: u64, digest: &str) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Inspect qplay update: {error}")),
    };
    if !metadata.is_file() || metadata.len() != size {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|error| format!("Read qplay update: {error}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Read qplay update: {error}"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()) == digest)
}

fn run_network<T>(
    cancel: &AtomicBool,
    operation: impl Future<Output = Result<T, String>>,
) -> Result<Option<T>, String> {
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Start qplay update network: {error}"))?;
    let result = runtime.block_on(async {
        tokio::select! {
            biased;
            _ = wait_for_cancel(cancel) => Ok(None),
            result = operation => result.map(Some),
        }
    });
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

async fn wait_for_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .user_agent(format!("qplay/{}", VERSION.trim()))
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(timeout)
        .build()
        .map_err(|error| format!("Create qplay update client: {error}"))
}

fn parse_version(value: &str) -> Option<(u32, u32, u32)> {
    let mut parts = value.split('.');
    let result = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(result)
}

fn valid_digest(value: &str) -> Option<&str> {
    let digest = value.strip_prefix("sha256:")?;
    (digest.len() == 64
        && digest
            .bytes()
            .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase()))
    .then_some(digest)
}
