use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use super::{ManifestFile, check_cancel};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(1);

pub fn available_bytes(path: &Path) -> Result<u64, String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "Kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0;
        if unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "Inspect GPU runtime free space: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(available)
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|error| error.to_string())?;
        let mut status = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), status.as_mut_ptr()) } != 0 {
            return Err(format!(
                "Inspect GPU runtime free space: {}",
                std::io::Error::last_os_error()
            ));
        }
        let status = unsafe { status.assume_init() };
        Ok(status.f_bavail.saturating_mul(status.f_frsize))
    }
}

pub fn normal_directory(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("Inspect GPU directory: {error}"))?;
    if !metadata.is_dir() || crate::files::is_link(&metadata) {
        return Err("The GPU runtime directory is not a normal directory.".into());
    }
    Ok(())
}

pub fn normal_file(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("Inspect GPU file: {error}"))?;
    if !metadata.is_file() || crate::files::is_link(&metadata) {
        return Err("The GPU runtime file is not a normal file.".into());
    }
    Ok(())
}

pub fn safe_relative(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.contains([':', '\\', '\0'])
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("The GPU manifest contains an unsafe path.".into());
    }
    Ok(())
}

pub fn reject_links(root: &Path, path: &Path) -> Result<(), String> {
    let mut current = path;
    while current != root {
        let metadata = fs::symlink_metadata(current)
            .map_err(|error| format!("Inspect GPU runtime path: {error}"))?;
        if crate::files::is_link(&metadata) {
            return Err("The GPU runtime contains a link.".into());
        }
        current = current
            .parent()
            .ok_or("The GPU path escaped its runtime.")?;
    }
    Ok(())
}

pub fn collect_files(
    root: &Path,
    cancel: &AtomicBool,
) -> Result<BTreeMap<String, ManifestFile>, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<String, ManifestFile>,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            check_cancel(cancel)?;
            let path = entry.map_err(|error| error.to_string())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if crate::files::is_link(&metadata) {
                return Err("The staged GPU runtime contains a link.".into());
            }
            if metadata.is_dir() {
                visit(root, &path, files, cancel)?;
                continue;
            }
            normal_file(&path)?;
            let mut input = File::open(&path).map_err(|error| error.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                check_cancel(cancel)?;
                let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(
                relative,
                ManifestFile {
                    size: metadata.len(),
                    sha256: format!("{:x}", hash.finalize()),
                },
            );
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files, cancel)?;
    Ok(files)
}

pub struct Staging {
    pub path: PathBuf,
}

impl Staging {
    pub fn directory(parent: &Path) -> Result<Self, String> {
        normal_directory(parent)?;
        let path = parent.join(format!(
            "native-{}-{}",
            std::process::id(),
            NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).map_err(|error| format!("Create private GPU staging: {error}"))?;
        Ok(Self { path })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub fn replace_manifest(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "Kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
        }
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(format!(
                "Publish GPU manifest: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    #[cfg(not(windows))]
    fs::rename(source, destination).map_err(|error| format!("Publish GPU manifest: {error}"))?;
    Ok(())
}
