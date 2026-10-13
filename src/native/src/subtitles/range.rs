//! Bounded reads of immutable HTTP representations for indexed Matroska cues.

use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_MATCH, LOCATION, RANGE,
};
use reqwest::{Client, StatusCode, Url};
use tokio::sync::watch;

use super::SubtitleSourceError;
use super::network::{http_url, scoped_headers};

const PAGE_BYTES: u64 = 64 * 1024;
const MAX_RECEIVED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_REQUESTS: u32 = 512;
const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(super) struct RangeMedia {
    client: Client,
    source: Url,
    url: Url,
    headers: Vec<String>,
    pub(super) etag: reqwest::header::HeaderValue,
    pub(super) length: u64,
}

pub(super) async fn probe_range(
    source: &str,
    headers: &[String],
    http_proxy: Option<&str>,
    mut cancelled: watch::Receiver<bool>,
) -> Result<RangeMedia, SubtitleSourceError> {
    if *cancelled.borrow() {
        return Err(SubtitleSourceError::Cancelled);
    }
    let source = http_url(source)?;
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
        let mut target = source.clone();
        for redirects in 0..=5 {
            let scoped = scoped_headers(Some(&source), &target, headers)?;
            let response = client
                .head(target.clone())
                .headers(scoped)
                .header(ACCEPT_ENCODING, "identity")
                .send()
                .await
                .map_err(|_| SubtitleSourceError::Source)?;
            if response.status().is_redirection() {
                if redirects == 5 {
                    return Err(SubtitleSourceError::Source);
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(SubtitleSourceError::Source)?;
                let next = http_url(
                    target
                        .join(location)
                        .map_err(|_| SubtitleSourceError::Source)?
                        .as_str(),
                )?;
                if target.scheme() == "https" && next.scheme() != "https" {
                    return Err(SubtitleSourceError::Source);
                }
                target = next;
                continue;
            }
            if response.status() != StatusCode::OK {
                return Err(SubtitleSourceError::Source);
            }
            let length = response
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|length| *length > 0)
                .ok_or(SubtitleSourceError::Source)?;
            let etag = response
                .headers()
                .get(ETAG)
                .filter(|value| strong_etag(value.as_bytes()))
                .cloned()
                .ok_or(SubtitleSourceError::Source)?;
            return Ok(RangeMedia {
                client,
                source,
                url: target,
                headers: headers.to_vec(),
                etag,
                length,
            });
        }
        Err(SubtitleSourceError::Source)
    };
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(8), acquire) =>
            result.map_err(|_| SubtitleSourceError::Source)?,
        _ = cancelled.changed() => Err(SubtitleSourceError::Cancelled),
    }
}

pub(super) fn strong_etag(etag: &[u8]) -> bool {
    etag.len() >= 2
        && etag.first() == Some(&b'"')
        && etag.last() == Some(&b'"')
        && etag[1..etag.len() - 1]
            .iter()
            .all(|byte| matches!(*byte, 0x21 | 0x23..=0x7e | 0x80..=0xff))
}

/// Synchronous Read + Seek for the vendored demuxer. The worker owns the reader
/// until it has drained cancellation; at most one page is retained.
pub(super) struct MediaReader {
    range: RangeMedia,
    runtime: tokio::runtime::Handle,
    position: u64,
    page_start: u64,
    page: Vec<u8>,
    received: u64,
    requests: u32,
    deadline: Instant,
    cancelled: watch::Receiver<bool>,
    error: Option<SubtitleSourceError>,
}

impl MediaReader {
    pub(super) fn new(range: RangeMedia, cancelled: watch::Receiver<bool>) -> Self {
        Self {
            range,
            runtime: tokio::runtime::Handle::current(),
            position: 0,
            page_start: 0,
            page: Vec::new(),
            received: 0,
            requests: 0,
            deadline: Instant::now() + READ_TIMEOUT,
            cancelled,
            error: None,
        }
    }

    pub(super) fn take_error(&mut self) -> Option<SubtitleSourceError> {
        self.error.take()
    }

    fn fail<T>(&mut self, error: SubtitleSourceError) -> std::io::Result<T> {
        self.error = Some(error);
        Err(std::io::Error::other("Subtitle range acquisition stopped"))
    }

    fn check(&mut self) -> std::io::Result<()> {
        if *self.cancelled.borrow() {
            return self.fail(SubtitleSourceError::Cancelled);
        }
        if self.error.is_some() {
            return Err(std::io::Error::other("Subtitle range acquisition stopped"));
        }
        if Instant::now() >= self.deadline {
            return self.fail(SubtitleSourceError::Extraction);
        }
        Ok(())
    }
}

impl Read for MediaReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.check()?;
        if output.is_empty() || self.position == self.range.length {
            return Ok(0);
        }
        if self.position < self.page_start
            || self.position >= self.page_start + self.page.len() as u64
        {
            if self.requests >= MAX_REQUESTS || self.received >= MAX_RECEIVED_BYTES {
                return self.fail(SubtitleSourceError::TooLarge);
            }
            let size = PAGE_BYTES
                .min(self.range.length - self.position)
                .min(MAX_RECEIVED_BYTES - self.received);
            self.requests += 1;
            let result = self.runtime.block_on(read_page(
                &self.range,
                self.position,
                size,
                self.deadline,
                self.cancelled.clone(),
            ));
            match result {
                Ok(page) => {
                    self.page_start = self.position;
                    self.received += page.len() as u64;
                    self.page = page;
                }
                Err(error) => return self.fail(error),
            }
        }
        let offset = (self.position - self.page_start) as usize;
        let count = output.len().min(self.page.len() - offset);
        output[..count].copy_from_slice(&self.page[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for MediaReader {
    fn seek(&mut self, target: SeekFrom) -> std::io::Result<u64> {
        self.check()?;
        let position = match target {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.range.length) + i128::from(offset),
        };
        if position < 0 || position > i128::from(self.range.length) {
            return self.fail(SubtitleSourceError::Source);
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

async fn read_page(
    range: &RangeMedia,
    start: u64,
    size: u64,
    deadline: Instant,
    mut cancelled: watch::Receiver<bool>,
) -> Result<Vec<u8>, SubtitleSourceError> {
    let end = start + size - 1;
    let acquire = async {
        let headers = scoped_headers(Some(&range.source), &range.url, &range.headers)?;
        let mut response = range
            .client
            .get(range.url.clone())
            .headers(headers)
            .header(ACCEPT_ENCODING, "identity")
            .header(IF_MATCH, range.etag.clone())
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|_| SubtitleSourceError::Source)?;
        let expected_range = format!("bytes {start}-{end}/{}", range.length);
        if response.status() != StatusCode::PARTIAL_CONTENT
            || response.headers().get(ETAG) != Some(&range.etag)
            || response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                != Some(expected_range.as_str())
            || response.content_length() != Some(size)
        {
            return Err(SubtitleSourceError::Source);
        }
        let mut bytes = Vec::with_capacity(size as usize);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| SubtitleSourceError::Source)?
        {
            if chunk.len() > size as usize - bytes.len() {
                return Err(SubtitleSourceError::Source);
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() as u64 != size {
            return Err(SubtitleSourceError::Source);
        }
        Ok(bytes)
    };
    tokio::select! {
        result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), acquire) =>
            result.map_err(|_| SubtitleSourceError::Extraction)?,
        _ = cancelled.changed() => Err(SubtitleSourceError::Cancelled),
    }
}
