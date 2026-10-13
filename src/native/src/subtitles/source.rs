//! Local, caller-authorized text acquisition and verified FFmpeg mapping.

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use matroska_demuxer::{Frame, MatroskaFile, TrackType};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::timeout;

use super::{SubtitleDocument, SubtitleError, SubtitleFormat, SubtitleIdentity};

pub(super) const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PROBE_BYTES: u64 = 1024 * 1024;
const MAX_MAPPING_READ_BYTES: u64 = 128 * 1024 * 1024;
const MAX_MAPPING_FRAMES: usize = 20_000;
const MAX_MAPPING_SUBTITLE_FRAMES: usize = 64;
const EXTRACTION_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, thiserror::Error)]
pub enum SubtitleSourceError {
    #[error("The subtitle source is unsupported or could not be read")]
    Source,
    #[error("This video contains image subtitles; choose a text subtitle track")]
    Bitmap,
    #[error("The subtitle source exceeds its size limit")]
    TooLarge,
    #[error("The subtitle extraction was cancelled")]
    Cancelled,
    #[error("The verified media tool could not extract this track")]
    Extraction,
    #[error("The selected player track has no verified FFmpeg stream mapping")]
    TrackMapping,
    #[error(transparent)]
    Document(#[from] SubtitleError),
}

/// These paths come from the Player's verified qplay runtime-info response.
/// This module never discovers or installs a media runtime.
pub struct VerifiedMediaTools {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    runtime_dir: PathBuf,
}

impl VerifiedMediaTools {
    /// Resolve tools only from qplay's already verified immutable bundle.
    pub fn from_library(library: &Path) -> Result<Self, SubtitleSourceError> {
        let runtime_dir = library.parent().ok_or(SubtitleSourceError::Source)?;
        let bundle = runtime_dir.parent().ok_or(SubtitleSourceError::Source)?;
        crate::runtime_info::verify_bundle(bundle).map_err(|_| SubtitleSourceError::Source)?;
        Ok(Self {
            ffmpeg: crate::runtime_info::tool(runtime_dir, "ffmpeg.exe")
                .map_err(|_| SubtitleSourceError::Source)?,
            ffprobe: crate::runtime_info::tool(runtime_dir, "ffprobe.exe")
                .map_err(|_| SubtitleSourceError::Source)?,
            runtime_dir: runtime_dir.to_owned(),
        })
    }
}

/// Exact fields read from mpv's selected `track-list` row and `current-demuxer`.
pub struct MpvEmbeddedTrack {
    pub mpv_id: i64,
    pub ff_index: Option<i64>,
    pub src_id: Option<i64>,
    pub codec: String,
    pub current_demuxer: String,
    pub origin_ms: i64,
}

pub struct EmbeddedTextTrack {
    stream_index: u32,
    codec: String,
    origin_ms: i64,
    source_digest: String,
}

impl EmbeddedTextTrack {
    pub fn identity(&self, media_digest: String) -> SubtitleIdentity {
        SubtitleIdentity {
            media_digest,
            stream_index: Some(self.stream_index),
            source_digest: self.source_digest.clone(),
        }
    }

    fn text_codec(&self) -> Result<(), SubtitleSourceError> {
        if self.stream_index > 512 {
            return Err(SubtitleSourceError::Source);
        }
        validate_text_codec(&self.codec)
    }
}

pub(super) fn validate_text_codec(codec: &str) -> Result<(), SubtitleSourceError> {
    let codec = codec.to_ascii_lowercase();
    if matches!(
        codec.as_str(),
        "hdmv_pgs_subtitle"
            | "pgs"
            | "dvd_subtitle"
            | "dvdsub"
            | "dvb_subtitle"
            | "dvbsub"
            | "xsub"
    ) {
        return Err(SubtitleSourceError::Bitmap);
    }
    if matches!(
        codec.as_str(),
        "ass" | "ssa" | "subrip" | "srt" | "webvtt" | "mov_text" | "text"
    ) {
        return Ok(());
    }
    Err(SubtitleSourceError::Source)
}

/// Exact identity for an authorized local media file. Run off the GUI thread.
pub async fn digest_local_media(
    media: &Path,
    mut cancelled: watch::Receiver<bool>,
) -> Result<String, SubtitleSourceError> {
    if !local_path(media) || !regular_file(media).await? {
        return Err(SubtitleSourceError::Source);
    }
    let mut file = tokio::fs::File::open(media)
        .await
        .map_err(|_| SubtitleSourceError::Source)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if *cancelled.borrow() {
            return Err(SubtitleSourceError::Cancelled);
        }
        let count = tokio::select! {
            read = file.read(&mut buffer) => read.map_err(|_| SubtitleSourceError::Source)?,
            _ = cancelled.changed() => return Err(SubtitleSourceError::Cancelled),
        };
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<ProbeStream>,
    #[serde(default)]
    packets: Vec<ProbePacket>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: u32,
    codec_type: String,
    codec_name: String,
}

#[derive(Deserialize)]
struct ProbePacket {
    stream_index: u32,
    pts_time: String,
    data_hash: String,
}

/// mpv documents `ff-index` as potentially wrong outside libavformat.
/// Reject every mapping we cannot prove against qplay's own FFprobe.
pub async fn verify_embedded_track(
    media: &Path,
    tools: &VerifiedMediaTools,
    selected: &MpvEmbeddedTrack,
    mut cancelled: watch::Receiver<bool>,
) -> Result<EmbeddedTextTrack, SubtitleSourceError> {
    validate_text_codec(&selected.codec)?;
    if selected.mpv_id <= 0 || !matches!(selected.current_demuxer.as_str(), "lavf" | "mkv") {
        return Err(SubtitleSourceError::TrackMapping);
    }
    if !local_path(media) || !regular_file(media).await? {
        return Err(SubtitleSourceError::Source);
    }
    if *cancelled.borrow() {
        return Err(SubtitleSourceError::Cancelled);
    }

    let mut command = Command::new(&tools.ffprobe);
    command.current_dir(&tools.runtime_dir).args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-select_streams",
        "s",
        "-show_entries",
        if selected.current_demuxer == "mkv" {
            "stream=index,codec_type,codec_name:packet=stream_index,pts_time,data_hash"
        } else {
            "stream=index,codec_type,codec_name"
        },
        "-show_streams",
        "-of",
        "json",
    ]);
    if selected.current_demuxer == "mkv" {
        command.args([
            "-read_intervals",
            "%+120",
            "-show_packets",
            "-show_data_hash",
            "sha256",
        ]);
    }
    command
        .arg(media)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| SubtitleSourceError::Extraction)?;
    let stdout = child.stdout.take().ok_or(SubtitleSourceError::Extraction)?;
    let mut bytes = Vec::new();
    let read = async {
        stdout
            .take(MAX_PROBE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| SubtitleSourceError::Extraction)?;
        if bytes.len() as u64 > MAX_PROBE_BYTES {
            return Err(SubtitleSourceError::TooLarge);
        }
        let status = child
            .wait()
            .await
            .map_err(|_| SubtitleSourceError::Extraction)?;
        if !status.success() {
            return Err(SubtitleSourceError::Extraction);
        }
        Ok(())
    };
    tokio::select! {
        result = timeout(EXTRACTION_TIMEOUT, read) => {
            result.map_err(|_| SubtitleSourceError::Extraction)??;
        }
        _ = cancelled.changed() => return Err(SubtitleSourceError::Cancelled),
    }
    let probe: Probe =
        serde_json::from_slice(&bytes).map_err(|_| SubtitleSourceError::Extraction)?;
    let index = if selected.current_demuxer == "mkv" {
        matroska_stream_index(media, selected, &probe, cancelled)?
    } else {
        selected
            .ff_index
            .and_then(|index| u32::try_from(index).ok())
            .filter(|index| *index <= 512)
            .ok_or(SubtitleSourceError::TrackMapping)?
    };
    if probe.streams.len() > 512
        || probe
            .streams
            .iter()
            .filter(|stream| {
                stream.index == index
                    && stream.codec_type == "subtitle"
                    && stream.codec_name.eq_ignore_ascii_case(&selected.codec)
            })
            .count()
            != 1
    {
        return Err(SubtitleSourceError::TrackMapping);
    }
    let track = EmbeddedTextTrack {
        stream_index: index,
        codec: selected.codec.clone(),
        origin_ms: selected.origin_ms,
        source_digest: format!(
            "{:x}",
            Sha256::digest(format!(
                "{}:{}:{}:{}",
                index,
                selected.src_id.unwrap_or_default(),
                selected.codec.to_ascii_lowercase(),
                selected.origin_ms
            ))
        ),
    };
    track.text_codec()?;
    Ok(track)
}

fn matroska_stream_index(
    media: &Path,
    selected: &MpvEmbeddedTrack,
    probe: &Probe,
    cancelled: watch::Receiver<bool>,
) -> Result<u32, SubtitleSourceError> {
    let src_id = selected
        .src_id
        .and_then(|id| u64::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or(SubtitleSourceError::TrackMapping)?;
    let file = File::open(media).map_err(|_| SubtitleSourceError::TrackMapping)?;
    let file = LimitedMediaFile {
        file,
        remaining: MAX_MAPPING_READ_BYTES,
        cancelled: cancelled.clone(),
    };
    let mut matroska = MatroskaFile::open(file).map_err(|_| SubtitleSourceError::TrackMapping)?;
    if matroska
        .tracks()
        .iter()
        .filter(|track| {
            track.track_number().get() == src_id && track.track_type() == TrackType::Subtitle
        })
        .count()
        != 1
    {
        return Err(SubtitleSourceError::TrackMapping);
    }

    let scale = u128::from(matroska.info().timestamp_scale().get());
    let mut mapped_index = None;
    let mut matched = 0;
    let mut frame = Frame::default();
    for _ in 0..MAX_MAPPING_FRAMES {
        if !matroska
            .next_frame(&mut frame)
            .map_err(|_| SubtitleSourceError::TrackMapping)?
        {
            break;
        }
        if frame.track != src_id || frame.data.is_empty() {
            continue;
        }
        let pts_us = u128::from(frame.timestamp)
            .checked_mul(scale)
            .and_then(|nanoseconds| nanoseconds.checked_div(1_000))
            .ok_or(SubtitleSourceError::TrackMapping)?;
        if pts_us > 120_000_000 {
            break;
        }
        let hash = format!("SHA256:{:x}", Sha256::digest(&frame.data));
        let candidates: HashSet<_> = probe
            .packets
            .iter()
            .filter(|packet| {
                packet.data_hash == hash
                    && packet
                        .pts_time
                        .parse::<f64>()
                        .ok()
                        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                        .is_some_and(|seconds| {
                            ((seconds * 1_000_000.0).round() as i128 - pts_us as i128).abs() <= 1
                        })
            })
            .map(|packet| packet.stream_index)
            .collect();
        if candidates.len() != 1 {
            continue;
        }
        let index = *candidates.iter().next().unwrap();
        if mapped_index.is_some_and(|previous| previous != index) {
            return Err(SubtitleSourceError::TrackMapping);
        }
        mapped_index = Some(index);
        matched += 1;
        if matched == MAX_MAPPING_SUBTITLE_FRAMES {
            break;
        }
    }
    if *cancelled.borrow() {
        return Err(SubtitleSourceError::Cancelled);
    }
    mapped_index.ok_or(SubtitleSourceError::TrackMapping)
}

struct LimitedMediaFile {
    file: File,
    remaining: u64,
    cancelled: watch::Receiver<bool>,
}

impl Read for LimitedMediaFile {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if *self.cancelled.borrow() || self.remaining == 0 {
            return Err(std::io::Error::other("Matroska mapping stopped"));
        }
        let limit = buffer.len().min(self.remaining as usize);
        let count = self.file.read(&mut buffer[..limit])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

impl Seek for LimitedMediaFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

pub async fn read_local_external(
    path: &Path,
    format: SubtitleFormat,
) -> Result<(SubtitleDocument, String), SubtitleSourceError> {
    if !local_path(path) {
        return Err(SubtitleSourceError::Source);
    }
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| SubtitleSourceError::Source)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(SubtitleSourceError::Source);
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(SubtitleSourceError::TooLarge);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| SubtitleSourceError::Source)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(SubtitleSourceError::TooLarge);
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let text = decode_text(&bytes)?;
    Ok((SubtitleDocument::parse(&text, format)?, digest))
}

pub async fn extract_local_embedded(
    media: &Path,
    tools: &VerifiedMediaTools,
    track: &EmbeddedTextTrack,
    start_ms: i64,
    end_ms: i64,
    mut cancelled: watch::Receiver<bool>,
) -> Result<SubtitleDocument, SubtitleSourceError> {
    track.text_codec()?;
    if *cancelled.borrow() {
        return Err(SubtitleSourceError::Cancelled);
    }
    if start_ms < 0
        || start_ms > 7 * 86_400_000
        || end_ms <= start_ms
        || end_ms - start_ms > 120_000
        || !(-1_000_000_000..=1_000_000_000).contains(&track.origin_ms)
        || !local_path(media)
        || !local_path(&tools.ffmpeg)
        || !local_path(&tools.runtime_dir)
    {
        return Err(SubtitleSourceError::Source);
    }
    for path in [media, tools.ffmpeg.as_path()] {
        if !regular_file(path).await? {
            return Err(SubtitleSourceError::Source);
        }
    }
    if !tools.runtime_dir.is_dir() {
        return Err(SubtitleSourceError::Source);
    }

    let start_seconds = (start_ms + track.origin_ms) as f64 / 1000.0;
    let duration_seconds = (end_ms - start_ms) as f64 / 1000.0;
    let origin_seconds = -(track.origin_ms as f64 / 1000.0);
    let mut command = Command::new(&tools.ffmpeg);
    let authored =
        track.codec.eq_ignore_ascii_case("ass") || track.codec.eq_ignore_ascii_case("ssa");
    let (subtitle_codec, output_format, document_format) = if authored {
        ("copy", "ass", SubtitleFormat::Ass)
    } else {
        ("srt", "srt", SubtitleFormat::Srt)
    };
    command
        .current_dir(&tools.runtime_dir)
        .args([
            "-nostdin",
            "-hide_banner",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            "matroska,webm,mov,mp4,mpegts,avi,ogg,flv,mpeg,mxf,asf,rm",
            "-seek_timestamp",
            "1",
            "-ss",
        ])
        .arg(start_seconds.to_string())
        .arg("-copyts")
        .arg("-i")
        .arg(media)
        .arg("-t")
        .arg(duration_seconds.to_string())
        .arg("-output_ts_offset")
        .arg(origin_seconds.to_string())
        .args(["-avoid_negative_ts", "disabled", "-map"])
        .arg(format!("0:{}", track.stream_index))
        .args([
            "-vn",
            "-an",
            "-c:s",
            subtitle_codec,
            "-f",
            output_format,
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|_| SubtitleSourceError::Extraction)?;
    let stdout = child.stdout.take().ok_or(SubtitleSourceError::Extraction)?;
    let mut bytes = Vec::new();
    let read = async {
        stdout
            .take(MAX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| SubtitleSourceError::Extraction)?;
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err(SubtitleSourceError::TooLarge);
        }
        let status = child
            .wait()
            .await
            .map_err(|_| SubtitleSourceError::Extraction)?;
        if !status.success() {
            return Err(SubtitleSourceError::Extraction);
        }
        Ok(())
    };
    tokio::select! {
        result = timeout(EXTRACTION_TIMEOUT, read) => {
            result.map_err(|_| SubtitleSourceError::Extraction)??;
        }
        _ = cancelled.changed() => return Err(SubtitleSourceError::Cancelled),
    }
    let text = decode_text(&bytes)?;
    Ok(SubtitleDocument::parse(&text, document_format)?)
}

pub(super) fn local_path(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        matches!(
            path.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        )
    }
    #[cfg(not(windows))]
    {
        true
    }
}

async fn regular_file(path: &Path) -> Result<bool, SubtitleSourceError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|_| SubtitleSourceError::Source)?;
    Ok(metadata.is_file() && !metadata.file_type().is_symlink())
}

pub(super) fn decode_text(bytes: &[u8]) -> Result<String, SubtitleSourceError> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        let units: Vec<_> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| {
                if little {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        if bytes.len() % 2 != 0 {
            return Err(SubtitleSourceError::Source);
        }
        return String::from_utf16(&units).map_err(|_| SubtitleSourceError::Source);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    String::from_utf8(bytes.to_vec()).map_err(|_| SubtitleSourceError::Source)
}
