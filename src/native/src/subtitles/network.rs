//! Acquisition of an already selected external HTTP text track.
//! qbutt's guarded gateway retains ownership of protected upstream requests.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use reqwest::{Client, Url};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio::time::timeout;

use super::source::{MAX_SOURCE_BYTES, decode_text};
use super::{SubtitleDocument, SubtitleFormat, SubtitleSourceError};

const TEXT_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_REDIRECTS: usize = 5;

pub(super) async fn read_network_external(
    media: &str,
    source: &str,
    headers: &[String],
    http_proxy: Option<&str>,
    format: SubtitleFormat,
    mut cancelled: watch::Receiver<bool>,
) -> Result<(SubtitleDocument, String), SubtitleSourceError> {
    if *cancelled.borrow() {
        return Err(SubtitleSourceError::Cancelled);
    }
    let media = Url::parse(media).ok();
    let mut source = http_url(source)?;
    let builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5));
    let builder = if source.host_str() == Some("127.0.0.1") {
        builder.no_proxy()
    } else if let Some(proxy) = http_proxy.filter(|proxy| !proxy.is_empty()) {
        builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| SubtitleSourceError::Source)?)
    } else {
        builder
    };
    let client = builder.build().map_err(|_| SubtitleSourceError::Source)?;

    let acquire = async {
        for redirects in 0..=MAX_REDIRECTS {
            let scoped = scoped_headers(media.as_ref(), &source, headers)?;
            let mut response = client
                .get(source.clone())
                .headers(scoped)
                .send()
                .await
                .map_err(|_| SubtitleSourceError::Source)?;
            if response.status().is_redirection() {
                if redirects == MAX_REDIRECTS {
                    return Err(SubtitleSourceError::Source);
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(SubtitleSourceError::Source)?;
                let redirected = source
                    .join(location)
                    .map_err(|_| SubtitleSourceError::Source)?;
                let redirected = http_url(redirected.as_str())?;
                if source.scheme() == "https" && redirected.scheme() != "https" {
                    return Err(SubtitleSourceError::Source);
                }
                source = redirected;
                continue;
            }
            if !response.status().is_success() {
                return Err(SubtitleSourceError::Source);
            }
            if response
                .content_length()
                .is_some_and(|size| size > MAX_SOURCE_BYTES)
            {
                return Err(SubtitleSourceError::TooLarge);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| SubtitleSourceError::Source)?
            {
                if chunk.len() as u64 > MAX_SOURCE_BYTES.saturating_sub(bytes.len() as u64) {
                    return Err(SubtitleSourceError::TooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let text = decode_text(&bytes)?;
            return Ok((SubtitleDocument::parse(&text, format)?, digest));
        }
        Err(SubtitleSourceError::Source)
    };
    tokio::select! {
        result = timeout(TEXT_TIMEOUT, acquire) => {
            result.map_err(|_| SubtitleSourceError::Source)?
        }
        _ = cancelled.changed() => Err(SubtitleSourceError::Cancelled),
    }
}

pub(super) fn http_url(value: &str) -> Result<Url, SubtitleSourceError> {
    let url = Url::parse(value).map_err(|_| SubtitleSourceError::Source)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return Err(SubtitleSourceError::Source);
    }
    Ok(url)
}

pub(super) fn scoped_headers(
    media: Option<&Url>,
    target: &Url,
    headers: &[String],
) -> Result<HeaderMap, SubtitleSourceError> {
    if headers.len() > 64 || headers.iter().map(String::len).sum::<usize>() > 64 * 1024 {
        return Err(SubtitleSourceError::Source);
    }
    let same_origin = media.is_some_and(|media| media.origin() == target.origin());
    let mut scoped = HeaderMap::new();
    for header in headers {
        let (name, value) = header.split_once(':').ok_or(SubtitleSourceError::Source)?;
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| SubtitleSourceError::Source)?;
        let value = HeaderValue::from_str(value.trim()).map_err(|_| SubtitleSourceError::Source)?;
        if same_origin
            || matches!(
                name.as_str(),
                "user-agent" | "referer" | "origin" | "accept" | "accept-language"
            )
        {
            scoped.append(name, value);
        }
    }
    Ok(scoped)
}
