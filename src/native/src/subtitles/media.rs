//! Playback-scoped source, authorization snapshot and translation cache identity.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT_ENCODING, CONTENT_LENGTH, ETAG};
use reqwest::{Client, StatusCode, Url};
use sha2::{Digest, Sha256};
use tokio::sync::{OnceCell, watch};

use super::network::{http_url, read_network_external, scoped_headers};
use super::source::{digest_local_media, local_path, read_local_external};
use super::{SubtitleDocument, SubtitleFormat, SubtitleSourceError};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// Replaced on every load, including reopen. Unknown remote identities are
/// session-scoped; an expiring URL or authorization token is never a digest.
pub struct PlaybackMedia {
    source: String,
    headers: Vec<String>,
    http_proxy: Option<String>,
    digest: OnceCell<String>,
    session_digest: String,
}

impl PlaybackMedia {
    pub fn new(source: String, headers: Vec<String>, http_proxy: Option<String>) -> Self {
        let nonce = format!(
            "qplay-subtitle-session:{}:{}:{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
        );
        Self {
            source,
            headers,
            http_proxy,
            digest: OnceCell::new(),
            session_digest: format!("{:x}", Sha256::digest(nonce)),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        let path = Path::new(&self.source);
        local_path(path).then_some(path)
    }

    pub async fn get(
        &self,
        mut cancelled: watch::Receiver<bool>,
    ) -> Result<String, SubtitleSourceError> {
        if *cancelled.borrow() {
            return Err(SubtitleSourceError::Cancelled);
        }
        let acquisition_cancelled = cancelled.clone();
        tokio::select! {
            result = self.digest.get_or_try_init(|| async {
                if let Some(path) = self.path() {
                    return digest_local_media(path, acquisition_cancelled).await;
                }
                let identity = match torrent_range_url(&self.source) {
                    Some(url) => range_identity(url, &self.headers, acquisition_cancelled).await?,
                    None => None,
                };
                Ok(identity.unwrap_or_else(|| self.session_digest.clone()))
            }) => result.cloned(),
            _ = cancelled.changed() => Err(SubtitleSourceError::Cancelled),
        }
    }

    pub async fn read_external(
        &self,
        source: &str,
        format: SubtitleFormat,
        cancelled: watch::Receiver<bool>,
    ) -> Result<(SubtitleDocument, String), SubtitleSourceError> {
        if http_url(source).is_ok() {
            read_network_external(
                &self.source,
                source,
                &self.headers,
                self.http_proxy.as_deref(),
                format,
                cancelled,
            )
            .await
        } else {
            if *cancelled.borrow() {
                return Err(SubtitleSourceError::Cancelled);
            }
            read_local_external(Path::new(source), format).await
        }
    }
}

/// Only qbutt's private verified torrent endpoint has an immutable ETag contract.
fn torrent_range_url(source: &str) -> Option<Url> {
    let url = http_url(source).ok()?;
    let parts = url.path_segments()?.collect::<Vec<_>>();
    (url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.query().is_none()
        && url.fragment().is_none()
        && parts.len() == 3
        && parts[0] == "media"
        && parts[1].len() == 32
        && parts[1].bytes().all(|byte| byte.is_ascii_hexdigit())
        && !parts[2].is_empty())
    .then_some(url)
}

async fn range_identity(
    url: Url,
    headers: &[String],
    mut cancelled: watch::Receiver<bool>,
) -> Result<Option<String>, SubtitleSourceError> {
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| SubtitleSourceError::Source)?;
    let headers = scoped_headers(Some(&url), &url, headers)?;
    let request = async {
        let response = client
            .head(url)
            .headers(headers)
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .ok()?;
        if response.status() != StatusCode::OK {
            return None;
        }
        let length = response
            .headers()
            .get(CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .parse::<u64>()
            .ok()?;
        let etag = response.headers().get(ETAG)?.as_bytes();
        if length == 0
            || etag.len() < 2
            || etag.first() != Some(&b'"')
            || etag.last() != Some(&b'"')
            || etag[1..etag.len() - 1]
                .iter()
                .any(|byte| !matches!(*byte, 0x21 | 0x23..=0x7e | 0x80..=0xff))
        {
            return None;
        }
        let mut digest = Sha256::new();
        digest.update(b"qbutt-immutable-range-v1\0");
        digest.update(etag);
        digest.update(length.to_be_bytes());
        Some(format!("{:x}", digest.finalize()))
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(5), request) => Ok(result.ok().flatten()),
        _ = cancelled.changed() => Err(SubtitleSourceError::Cancelled),
    }
}
