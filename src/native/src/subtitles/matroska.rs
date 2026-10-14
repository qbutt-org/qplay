//! Bounded embedded text acquisition without reading the video payload.

use std::sync::Arc;

use matroska_demuxer::{Frame, MatroskaFile, TrackType};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use super::source::MAX_SOURCE_BYTES;
use super::{
    MpvEmbeddedTrack, PlaybackMedia, SubtitleDocument, SubtitleFormat, SubtitleIdentity,
    SubtitleSourceError,
};

const MAX_WINDOW_CUES: usize = 2_000;

pub async fn extract_network_matroska(
    media: Arc<PlaybackMedia>,
    selected: MpvEmbeddedTrack,
    start_ms: i64,
    end_ms: i64,
    media_digest: String,
    cancelled: watch::Receiver<bool>,
) -> Result<(SubtitleDocument, SubtitleIdentity), SubtitleSourceError> {
    if selected.mpv_id <= 0
        || selected.current_demuxer != "mkv"
        || start_ms < 0
        || start_ms > 7 * 86_400_000
        || end_ms <= start_ms
        || end_ms - start_ms > 120_000
        || !(-1_000_000_000..=1_000_000_000).contains(&selected.origin_ms)
    {
        return Err(SubtitleSourceError::TrackMapping);
    }
    let track_number = selected
        .src_id
        .and_then(|id| u64::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or(SubtitleSourceError::TrackMapping)?;
    let mut reader = media.reader(&media_digest, cancelled).await?;
    tokio::task::spawn_blocking(move || {
        let result = extract(
            &mut reader,
            &selected,
            track_number,
            start_ms,
            end_ms,
            media_digest,
        );
        match reader.take_error() {
            Some(error) => Err(error),
            None => result,
        }
    })
    .await
    .map_err(|_| SubtitleSourceError::Extraction)?
}

fn extract(
    reader: &mut super::range::MediaReader,
    selected: &MpvEmbeddedTrack,
    track_number: u64,
    start_ms: i64,
    end_ms: i64,
    media_digest: String,
) -> Result<(SubtitleDocument, SubtitleIdentity), SubtitleSourceError> {
    let mut matroska = MatroskaFile::open(reader).map_err(|_| SubtitleSourceError::Extraction)?;
    let matches = matroska
        .tracks()
        .iter()
        .filter(|track| track.track_number().get() == track_number)
        .collect::<Vec<_>>();
    if matches.len() != 1 || matches[0].track_type() != TrackType::Subtitle {
        return Err(SubtitleSourceError::TrackMapping);
    }
    let track = matches[0];
    let format = match (
        track.codec_id(),
        selected.codec.to_ascii_lowercase().as_str(),
    ) {
        ("S_TEXT/ASS", "ass") => SubtitleFormat::Ass,
        ("S_TEXT/SSA", "ssa" | "ass") => SubtitleFormat::Ssa,
        ("S_TEXT/UTF8", "subrip" | "srt" | "text") => SubtitleFormat::Srt,
        ("S_TEXT/WEBVTT", "webvtt") => SubtitleFormat::Vtt,
        ("S_HDMV/PGS" | "S_VOBSUB", _) => return Err(SubtitleSourceError::Bitmap),
        _ => return Err(SubtitleSourceError::TrackMapping),
    };
    if track
        .content_encodings()
        .is_some_and(|encodings| !encodings.is_empty())
    {
        return Err(SubtitleSourceError::Source);
    }
    let scale = u128::from(matroska.info().timestamp_scale().get());
    let delay = i128::from(track.codec_delay().unwrap_or(0));
    let codec_private = track.codec_private().unwrap_or_default();
    let mut source_digest = Sha256::new();
    source_digest.update(b"matroska-text-track-v1\0");
    source_digest.update(track_number.to_be_bytes());
    source_digest.update(track.codec_id().as_bytes());
    source_digest.update(codec_private);
    source_digest.update(selected.origin_ms.to_be_bytes());
    let identity = SubtitleIdentity {
        media_digest,
        stream_index: None,
        source_digest: format!("{:x}", source_digest.finalize()),
    };

    let mut output = match format {
        SubtitleFormat::Ass | SubtitleFormat::Ssa => {
            let header =
                std::str::from_utf8(codec_private).map_err(|_| SubtitleSourceError::Source)?;
            let header = header
                .split("[Events]")
                .next()
                .unwrap_or(header)
                .trim_end_matches('\0');
            let first = if format == SubtitleFormat::Ssa {
                "Marked"
            } else {
                "Layer"
            };
            format!(
                "{}\n\n[Events]\nFormat: {first}, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
                header.trim_end()
            )
        }
        SubtitleFormat::Vtt => {
            let header =
                std::str::from_utf8(codec_private).map_err(|_| SubtitleSourceError::Source)?;
            if header.starts_with("WEBVTT") {
                format!("{}\n\n", header.trim_end())
            } else {
                "WEBVTT\n\n".to_owned()
            }
        }
        SubtitleFormat::Srt => String::new(),
    };
    if output.len() as u64 > MAX_SOURCE_BYTES {
        return Err(SubtitleSourceError::TooLarge);
    }

    let cues = matroska
        .subtitle_cues(track_number)
        .map_err(|_| SubtitleSourceError::Extraction)?;
    let mut cue_count = 0;
    let mut frame = Frame::default();
    for cue in cues {
        let timestamp_ns = u128::from(cue.timestamp)
            .checked_mul(scale)
            .ok_or(SubtitleSourceError::Source)?;
        let duration_ns = u128::from(cue.duration)
            .checked_mul(scale)
            .ok_or(SubtitleSourceError::Source)?;
        let start_ns = i128::try_from(timestamp_ns).map_err(|_| SubtitleSourceError::Source)?
            - delay
            - i128::from(selected.origin_ms) * 1_000_000;
        let end_ns = start_ns
            .checked_add(i128::try_from(duration_ns).map_err(|_| SubtitleSourceError::Source)?)
            .ok_or(SubtitleSourceError::Source)?;
        let start = i64::try_from(start_ns.div_euclid(1_000_000))
            .map_err(|_| SubtitleSourceError::Source)?;
        let end =
            i64::try_from(end_ns.div_euclid(1_000_000)).map_err(|_| SubtitleSourceError::Source)?;
        if end <= start_ms || start >= end_ms {
            continue;
        }
        if start < 0 || end <= start || end > 7 * 86_400_000 {
            return Err(SubtitleSourceError::Source);
        }
        if cue_count >= MAX_WINDOW_CUES {
            return Err(SubtitleSourceError::TooLarge);
        }
        let addition = matroska
            .read_subtitle_cue(cue, &mut frame)
            .map_err(|_| SubtitleSourceError::Extraction)?;
        let packet = std::str::from_utf8(&frame.data).map_err(|_| SubtitleSourceError::Source)?;
        cue_count += 1;
        let text = match format {
            SubtitleFormat::Ass | SubtitleFormat::Ssa => {
                let fields = packet.splitn(9, ',').collect::<Vec<_>>();
                if fields.len() != 9 || fields[..8].iter().any(|field| field.contains(['\r', '\n']))
                {
                    return Err(SubtitleSourceError::Source);
                }
                let layer = if format == SubtitleFormat::Ssa {
                    "Marked=0"
                } else {
                    fields[1]
                };
                format!(
                    "Dialogue: {layer},{},{},{},{},{},{},{},{},{}\n",
                    timestamp(start, true),
                    timestamp(end, true),
                    fields[2],
                    fields[3],
                    fields[4],
                    fields[5],
                    fields[6],
                    fields[7],
                    fields[8]
                )
            }
            SubtitleFormat::Srt => format!(
                "{cue_count}\n{} --> {}\n{packet}\n\n",
                timestamp(start, false),
                timestamp(end, false)
            ),
            SubtitleFormat::Vtt => {
                let packet = restore_vtt_timestamps(packet, start)?;
                let mut prefix = String::new();
                let mut settings = String::new();
                if let Some(addition) = addition {
                    let addition =
                        std::str::from_utf8(&addition).map_err(|_| SubtitleSourceError::Source)?;
                    let (cue_settings, rest) = addition
                        .split_once('\n')
                        .ok_or(SubtitleSourceError::Source)?;
                    let (identifier, comments) =
                        rest.split_once('\n').ok_or(SubtitleSourceError::Source)?;
                    if cue_settings.contains('\r') || identifier.contains('\r') {
                        return Err(SubtitleSourceError::Source);
                    }
                    settings = cue_settings.to_owned();
                    if !comments.is_empty() {
                        prefix.push_str(comments.trim_end_matches('\n'));
                        prefix.push_str("\n\n");
                    }
                    if !identifier.is_empty() {
                        prefix.push_str(identifier);
                        prefix.push('\n');
                    }
                }
                let settings = if settings.is_empty() {
                    String::new()
                } else {
                    format!(" {settings}")
                };
                format!(
                    "{prefix}{} --> {}{settings}\n{packet}\n\n",
                    timestamp_vtt(start),
                    timestamp_vtt(end)
                )
            }
        };
        if text.len() as u64 > MAX_SOURCE_BYTES.saturating_sub(output.len() as u64) {
            return Err(SubtitleSourceError::TooLarge);
        }
        output.push_str(&text);
    }
    Ok((SubtitleDocument::parse(&output, format)?, identity))
}

fn timestamp(milliseconds: i64, ass: bool) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds / 60_000 % 60;
    let seconds = milliseconds / 1000 % 60;
    if ass {
        format!(
            "{hours}:{minutes:02}:{seconds:02}.{:02}",
            milliseconds / 10 % 100
        )
    } else {
        format!(
            "{hours:02}:{minutes:02}:{seconds:02},{:03}",
            milliseconds % 1000
        )
    }
}

fn timestamp_vtt(milliseconds: i64) -> String {
    timestamp(milliseconds, false).replace(',', ".")
}

fn restore_vtt_timestamps(packet: &str, start_ms: i64) -> Result<String, SubtitleSourceError> {
    let mut output = String::with_capacity(packet.len());
    let mut remaining = packet;
    while let Some(open) = remaining.find('<') {
        output.push_str(&remaining[..open]);
        let tag = &remaining[open..];
        let Some(close) = tag.find('>') else {
            output.push_str(tag);
            return Ok(output);
        };
        let value = &tag[1..close];
        if let Some(relative_ms) = parse_vtt_timestamp(value) {
            let absolute = start_ms
                .checked_add(relative_ms)
                .filter(|value| *value <= 7 * 86_400_000)
                .ok_or(SubtitleSourceError::Source)?;
            output.push('<');
            output.push_str(&timestamp_vtt(absolute));
            output.push('>');
        } else {
            output.push_str(&tag[..=close]);
        }
        remaining = &tag[close + 1..];
    }
    output.push_str(remaining);
    Ok(output)
}

fn parse_vtt_timestamp(value: &str) -> Option<i64> {
    let (clock, millis) = value.split_once('.')?;
    if millis.len() != 3 || !millis.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parts = clock.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.len() < 2 || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let seconds = parts.last()?.parse::<i64>().ok()?;
    let minutes = parts[parts.len() - 2].parse::<i64>().ok()?;
    if seconds >= 60 || parts.len() == 3 && minutes >= 60 {
        return None;
    }
    let hours = if parts.len() == 3 {
        parts[0].parse::<i64>().ok()?
    } else {
        0
    };
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(millis.parse::<i64>().ok()?)
}
