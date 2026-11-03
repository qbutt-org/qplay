use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::future::Future;
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(windows)]
pub fn move_new_file(source: &Path, target: &Path) -> std::io::Result<()> {
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileW(source: *const u16, target: *const u16) -> i32;
    }

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both UTF-16 paths are terminated and live through the Win32 call.
    if unsafe { MoveFileW(source.as_ptr(), target.as_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn move_new_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())?;
    let target = CString::new(target.as_os_str().as_bytes())?;
    // SAFETY: Valid terminated paths are borrowed for one synchronous syscall.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(windows)]
    let _ = path;
    Ok(())
}

pub fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(target_os = "linux")]
    {
        metadata.file_type().is_symlink()
    }
}

pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    expected_size: u64,
    expected_digest: &str,
    output: &mut File,
    progress: &mut impl FnMut(u64, u64),
    label: &str,
) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Download {label}: {error}"))?;
    let mut hasher = Sha256::new();
    let mut written = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Read {label} download: {error}"))?
    {
        written += chunk.len() as u64;
        if written > expected_size {
            return Err(format!("{label} download exceeds its expected size."));
        }
        hasher.update(&chunk);
        output
            .write_all(&chunk)
            .map_err(|error| format!("Stage {label}: {error}"))?;
        progress(written, expected_size);
    }
    if written != expected_size || format!("{:x}", hasher.finalize()) != expected_digest {
        return Err(format!("{label} download failed integrity verification."));
    }
    output
        .sync_all()
        .map_err(|error| format!("Flush {label}: {error}"))
}

pub fn verified_file(path: &Path, size: u64, digest: &str) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Inspect verified file: {error}")),
    };
    if !metadata.is_file() || metadata.len() != size || crate::files::is_link(&metadata) {
        return Ok(false);
    }
    Ok(hash_file(path)? == digest)
}

pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("Read verified file: {error}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Read verified file: {error}"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn run_network<T>(
    cancel: &AtomicBool,
    operation: impl Future<Output = Result<T, String>>,
) -> Result<Option<T>, String> {
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Start cancellable network operation: {error}"))?;
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
