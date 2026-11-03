use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// A persistent sentinel with a process-owned exclusive handle. Dropping it
/// releases ownership; a stale sentinel after a crash never blocks playback.
pub struct ExclusiveFileLock {
    _file: File,
}

impl ExclusiveFileLock {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        Self::try_acquire(path)?.ok_or("Frame generation is busy.".into())
    }

    /// Preparation workers wait for a canceled predecessor to finish native
    /// teardown. Graph construction remains nonblocking through `acquire`.
    pub fn wait(path: &Path, cancel: &AtomicBool) -> Result<Self, String> {
        loop {
            super::check_cancel(cancel)?;
            if let Some(lock) = Self::try_acquire(path)? {
                super::check_cancel(cancel)?;
                return Ok(lock);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn try_acquire(path: &Path) -> Result<Option<Self>, String> {
        let parent = path.parent().ok_or("The GPU lock has no directory.")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Create GPU lock directory: {error}"))?;
        super::normal_directory(parent)?;
        if let Ok(metadata) = fs::symlink_metadata(path)
            && (!metadata.is_file() || crate::files::is_link(&metadata))
        {
            return Err("The GPU lock is not a normal file.".into());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = match options.open(path) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Ok(None),
            Err(error) => return Err(format!("Open frame generation lock: {error}")),
        };
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: The descriptor remains owned by this guard until drop.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = std::io::Error::last_os_error();
                return if error.kind() == std::io::ErrorKind::WouldBlock {
                    Ok(None)
                } else {
                    Err(format!("Lock frame generation: {error}"))
                };
            }
        }
        Ok(Some(Self { _file: file }))
    }
}
