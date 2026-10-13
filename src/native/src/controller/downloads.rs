use std::pin::Pin;
use std::time::{Duration, Instant};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use super::ffi;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DownloadStatus {
    Ready,
    Pending(Instant),
    Queued,
    Failed,
}

pub(super) struct DownloadAction {
    token: String,
    path: String,
    status: DownloadStatus,
}

impl ffi::PlayerController {
    pub(super) fn download_state(&self) -> QString {
        QString::from(match self.download.as_ref().map(|action| action.status) {
            None => "unavailable",
            Some(DownloadStatus::Ready) => "ready",
            Some(DownloadStatus::Pending(_)) => "pending",
            Some(DownloadStatus::Queued) => "queued",
            Some(DownloadStatus::Failed) => "failed",
        })
    }

    pub(super) fn receive_download_message(mut self: Pin<&mut Self>, arguments: &[String]) {
        if !self.launch.ipc || !self.launch.external_downloads || arguments.len() != 3 {
            return;
        }
        let token = &arguments[1];
        if !token.parse::<u64>().is_ok_and(|token| token > 0) {
            return;
        }
        match arguments[0].as_str() {
            "qplay-download-target" => {
                let path = &arguments[2];
                if !self.file_loaded
                    || &self.observed_path != path
                    || !(path.starts_with("https://") || path.starts_with("http://"))
                    || self
                        .session
                        .as_ref()
                        .and_then(|session| session.text("path"))
                        .as_ref()
                        != Some(path)
                {
                    return;
                }
                if self
                    .download
                    .as_ref()
                    .is_some_and(|action| &action.token == token && &action.path == path)
                {
                    return;
                }
                self.as_mut().rust_mut().download = Some(DownloadAction {
                    token: token.clone(),
                    path: path.clone(),
                    status: DownloadStatus::Ready,
                });
            }
            "qplay-download-result" => {
                let mut state = self.as_mut().rust_mut();
                let Some(action) = state.download.as_mut() else {
                    return;
                };
                if &action.token != token || !matches!(action.status, DownloadStatus::Pending(_)) {
                    return;
                }
                action.status = match arguments[2].as_str() {
                    "queued" => DownloadStatus::Queued,
                    "failed" => DownloadStatus::Failed,
                    _ => return,
                };
            }
            _ => return,
        }
    }

    pub(super) fn request_download(mut self: Pin<&mut Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(action) = self.download.as_ref() else {
            return;
        };
        if !matches!(
            action.status,
            DownloadStatus::Ready | DownloadStatus::Failed
        ) || !self.file_loaded
            || self.observed_path != action.path
            || session.text("path").as_ref() != Some(&action.path)
        {
            return;
        }
        let status = if session
            .command(&[
                "script-message",
                "qplay-download",
                &action.token,
                &action.path,
            ])
            .is_ok()
        {
            DownloadStatus::Pending(Instant::now() + DOWNLOAD_TIMEOUT)
        } else {
            DownloadStatus::Failed
        };
        self.as_mut().rust_mut().download.as_mut().unwrap().status = status;
        if matches!(status, DownloadStatus::Pending(_)) {
            let token = QString::from(&self.download.as_ref().unwrap().token);
            ffi::schedule_download_timeout(
                self.as_mut(),
                &token,
                DOWNLOAD_TIMEOUT.as_millis() as i32,
            );
        }
        self.as_mut().state_changed();
    }

    pub(super) fn download_timeout(mut self: Pin<&mut Self>, token: &QString) {
        let mut state = self.as_mut().rust_mut();
        let Some(action) = state.download.as_mut() else {
            return;
        };
        if action.token == token.to_string()
            && matches!(action.status, DownloadStatus::Pending(deadline) if Instant::now() >= deadline)
        {
            action.status = DownloadStatus::Failed;
            drop(state);
            self.as_mut().state_changed();
        }
    }
}
