use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use super::ffi;
use crate::app_update::{self, CheckResult, Release};

enum Operation {
    Check,
    Download(Release, PathBuf),
}

pub struct UpdateTask {
    thread: Box<cxx_qt::CxxQtThread<ffi::PlayerController>>,
    cancel: Arc<AtomicBool>,
    operation: Operation,
}

#[expect(
    clippy::boxed_local,
    reason = "CXX transfers this opaque worker task in an owning Rust box"
)]
pub fn run_update_task(task: Box<UpdateTask>) {
    let thread = task.thread.clone();
    let cancel = task.cancel.clone();
    match task.operation {
        Operation::Check => {
            let result = app_update::check(&cancel);
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let _ = thread.queue(move |mut player| {
                let mut state = player.as_mut().rust_mut();
                state.update_cancel = None;
                match result {
                    Ok(None) => return,
                    Ok(Some(CheckResult::Unpublished)) => state.update_state = "unpublished".into(),
                    Ok(Some(CheckResult::Current)) => state.update_state = "current".into(),
                    Ok(Some(CheckResult::Available(release))) => {
                        state.update_version = QString::from(release.version.as_str());
                        state.update_release = Some(release);
                        state.update_state = "available".into();
                    }
                    Err(error) => {
                        state.update_error = QString::from(error);
                        state.update_state = "checkFailed".into();
                    }
                }
                player.as_mut().state_changed();
            });
        }
        Operation::Download(release, data) => {
            let mut last_progress = -1;
            let result = app_update::download(&release, &data, &cancel, |progress| {
                if progress == last_progress {
                    return;
                }
                last_progress = progress;
                let _ = thread.queue(move |mut player| {
                    player.as_mut().rust_mut().update_progress = progress;
                    player.as_mut().state_changed();
                });
            });
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let _ = thread.queue(move |mut player| {
                let mut state = player.as_mut().rust_mut();
                state.update_cancel = None;
                match result {
                    Ok(Some(path)) => {
                        state.update_installer = Some(path);
                        state.update_state = "ready".into();
                    }
                    Ok(None) => state.update_state = "available".into(),
                    Err(error) => {
                        state.update_error = QString::from(error);
                        state.update_state = "downloadFailed".into();
                    }
                }
                player.as_mut().state_changed();
            });
        }
    }
}

impl ffi::PlayerController {
    pub(super) fn update_state(&self) -> QString {
        self.update_state.clone()
    }

    pub(super) fn update_version(&self) -> QString {
        self.update_version.clone()
    }

    pub(super) fn update_url(&self) -> QString {
        self.update_release
            .as_ref()
            .map(|release| QString::from(release.portable_url.as_str()))
            .unwrap_or_default()
    }

    pub(super) fn update_error(&self) -> QString {
        self.update_error.clone()
    }

    pub(super) fn update_progress(&self) -> i32 {
        self.update_progress
    }

    pub(super) fn portable(&self) -> bool {
        self.portable
    }

    pub(super) fn configure_updates(
        mut self: Pin<&mut Self>,
        data: &QString,
        install: &QString,
        portable: bool,
    ) {
        let mut state = self.as_mut().rust_mut();
        state.update_data = Some(PathBuf::from(data.to_string()));
        state.install_directory = Some(PathBuf::from(install.to_string()));
        state.portable = portable;
        self.as_mut().state_changed();
    }

    pub(super) fn check_updates(mut self: Pin<&mut Self>) {
        if matches!(
            self.update_state.to_string().as_str(),
            "checking" | "downloading"
        ) {
            return;
        }
        let thread = self.qt_thread();
        let mut state = self.as_mut().rust_mut();
        state.update_state = "checking".into();
        state.update_version = QString::default();
        state.update_error = QString::default();
        state.update_release = None;
        state.update_installer = None;
        let cancel = Arc::new(AtomicBool::new(false));
        state.update_cancel = Some(cancel.clone());
        let task = UpdateTask {
            thread: Box::new(thread),
            cancel,
            operation: Operation::Check,
        };
        ffi::start_update_task(Box::new(task));
        self.as_mut().state_changed();
    }

    pub(super) fn download_update(mut self: Pin<&mut Self>) {
        if !matches!(
            self.update_state.to_string().as_str(),
            "available" | "downloadFailed" | "installFailed"
        ) || self.portable
        {
            return;
        }
        let (Some(release), Some(data)) = (self.update_release.clone(), self.update_data.clone())
        else {
            return;
        };
        let thread = self.qt_thread();
        let mut state = self.as_mut().rust_mut();
        state.update_progress = 0;
        state.update_error = QString::default();
        state.update_installer = None;
        state.update_state = "downloading".into();
        let cancel = Arc::new(AtomicBool::new(false));
        state.update_cancel = Some(cancel.clone());
        let task = UpdateTask {
            thread: Box::new(thread),
            cancel,
            operation: Operation::Download(release, data),
        };
        ffi::start_update_task(Box::new(task));
        self.as_mut().state_changed();
    }

    pub(super) fn install_update(mut self: Pin<&mut Self>) -> bool {
        if self.update_state.to_string() != "ready" || self.portable {
            return false;
        }
        let (Some(release), Some(path), Some(install)) = (
            &self.update_release,
            &self.update_installer,
            &self.install_directory,
        ) else {
            return false;
        };
        match app_update::start_installer(release, path, install) {
            Ok(()) => true,
            Err(error) => {
                let mut state = self.as_mut().rust_mut();
                state.update_error = QString::from(error);
                state.update_state = "installFailed".into();
                self.as_mut().state_changed();
                false
            }
        }
    }
}
