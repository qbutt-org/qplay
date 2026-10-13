use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::framegen::Rate;
use super::process::run_bounded;

const MAX_OUTPUT_BYTES: u64 = 8 * 1024 * 1024;

const SAMPLE_FRACTIONS: [f64; 4] = [0.12, 0.36, 0.62, 0.86];
const FRAMES_PER_SAMPLE: usize = 48;

#[derive(Deserialize)]
struct TimingDocument {
    streams: Vec<TimingStream>,
    #[serde(default)]
    frames: Vec<TimingFrame>,
    #[serde(default)]
    packets: Vec<TimingPacket>,
}

#[derive(Deserialize)]
struct TimingStream {
    avg_frame_rate: String,
    r_frame_rate: String,
    time_base: String,
}

#[derive(Deserialize)]
struct TimingFrame {
    best_effort_timestamp: Option<Value>,
}

#[derive(Deserialize)]
struct TimingPacket {
    pts: Option<Value>,
    dts: Option<Value>,
}

#[derive(Deserialize)]
struct ExtractorResult {
    status: i32,
    stdout: String,
}

#[derive(Default, Deserialize)]
struct ExtractedFormat {
    format: Option<String>,
    format_note: Option<String>,
    vcodec: Option<String>,
    url: Option<String>,
}

#[derive(Deserialize)]
struct ExtractedVideo {
    formats: Option<Vec<ExtractedFormat>>,
    requested_formats: Option<Vec<ExtractedFormat>>,
    requested_downloads: Option<Vec<ExtractedFormat>>,
    #[serde(flatten)]
    selected: ExtractedFormat,
}

pub struct ResolvedNetworkSource {
    pub url: String,
    pub video_stream: String,
}

pub fn resolve_network_source(
    stream_filename: &str,
    track_title: &str,
    ff_index: u32,
    ytdl_result: Option<&str>,
) -> Result<ResolvedNetworkSource, String> {
    if stream_filename.starts_with("http://") || stream_filename.starts_with("https://") {
        return Ok(ResolvedNetworkSource {
            url: stream_filename.to_owned(),
            video_stream: ff_index.to_string(),
        });
    }
    let result: ExtractorResult =
        serde_json::from_str(ytdl_result.ok_or("The selected stream has no extractor result.")?)
            .map_err(|_| "The selected stream has an invalid extractor result.")?;
    if result.status != 0 || result.stdout.len() > 4 * 1024 * 1024 {
        return Err("The selected stream could not be resolved for timing analysis.".into());
    }
    let document: ExtractedVideo = serde_json::from_str(&result.stdout)
        .map_err(|_| "The selected stream has invalid format metadata.")?;
    let root = [document.selected];
    let formats = if !track_title.is_empty() {
        document.formats.as_deref()
    } else {
        None
    }
    .or(document.requested_formats.as_deref())
    .or(document.requested_downloads.as_deref())
    .unwrap_or(&root);
    let mut selected = None;
    for (index, format) in formats.iter().enumerate() {
        let Some(url) = format.url.as_deref() else {
            continue;
        };
        let title = format
            .format
            .as_deref()
            .or(format.format_note.as_deref())
            .unwrap_or("");
        if format.vcodec.as_deref().is_none_or(|codec| codec == "none")
            || !url.starts_with("http://") && !url.starts_with("https://")
            || !stream_filename.contains(url)
            || !track_title.is_empty()
                && track_title != title
                && track_title != format!("{title} muxed-{}", index + 1)
        {
            continue;
        }
        if selected.is_some() {
            return Err("The selected network video format is ambiguous.".into());
        }
        selected = Some(url.to_owned());
    }
    selected
        .map(|url| ResolvedNetworkSource {
            url,
            video_stream: "v:0".into(),
        })
        .ok_or_else(|| "The selected network video format was not found.".into())
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub source_rate: Rate,
    pub variable: bool,
}

fn parse_rate(value: &str) -> Option<Rate> {
    let (num, den) = value.split_once('/')?;
    Rate::new(num.parse().ok()?, den.parse().ok()?)
}

fn timestamp(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn gcd(mut left: i64, mut right: i64) -> i64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.abs()
}

fn allowed_deltas(rate: Rate, time_base: Rate, quantum: i64) -> Option<(i64, i64)> {
    let nominal = 1.0 / (rate.fps() * time_base.fps());
    if !nominal.is_finite() {
        return None;
    }
    let quantized = nominal / quantum as f64;
    let rounded = quantized.round();
    if (quantized - rounded).abs() < 1e-6 {
        return Some((rounded as i64 * quantum, rounded as i64 * quantum));
    }
    Some((
        quantized.floor() as i64 * quantum,
        quantized.ceil() as i64 * quantum,
    ))
}

fn candidate_score(deltas: &[i64], rate: Rate, time_base: Rate, quantum: i64) -> Option<f64> {
    let (lower, upper) = allowed_deltas(rate, time_base, quantum)?;
    if lower <= 0 {
        return None;
    }
    let matches = deltas
        .iter()
        .filter(|delta| **delta == lower || **delta == upper)
        .count();
    Some(matches as f64 / deltas.len() as f64)
}

fn analyze(document: TimingDocument) -> Result<Timing, String> {
    let stream = document
        .streams
        .first()
        .ok_or("The timing probe found no video stream.")?;
    let time_base = parse_rate(&stream.time_base)
        .ok_or("The timing probe did not provide a valid time base.")?;
    let r_rate = parse_rate(&stream.r_frame_rate);
    let avg_rate = parse_rate(&stream.avg_frame_rate);

    let local_frames = document.packets.is_empty();
    let timestamps: Vec<Option<i64>>;
    let sample_size;
    if local_frames {
        timestamps = document
            .frames
            .iter()
            .map(|frame| frame.best_effort_timestamp.as_ref().and_then(timestamp))
            .collect();
        sample_size = FRAMES_PER_SAMPLE;
    } else {
        let mut packet_times = Vec::with_capacity(document.packets.len());
        let mut last_dts = None;
        for packet in &document.packets {
            let pts = packet
                .pts
                .as_ref()
                .and_then(timestamp)
                .ok_or("The network timing probe omitted packet PTS.")?;
            packet_times.push(pts);
            if let Some(dts) = packet.dts.as_ref().and_then(timestamp) {
                last_dts = Some(last_dts.map_or(dts, |last: i64| last.max(dts)));
            }
        }
        packet_times.sort_unstable();
        if let Some(last_dts) = last_dts {
            packet_times.retain(|pts| *pts <= last_dts);
        }
        timestamps = packet_times.into_iter().map(Some).collect();
        sample_size = timestamps.len().max(1);
    }

    let mut deltas = Vec::new();
    for burst in timestamps.chunks(sample_size) {
        let mut previous = None;
        // Packet-count intervals can end before FFprobe flushes reordered B frames.
        let reliable_end = if local_frames {
            burst.len().saturating_sub(4)
        } else {
            burst.len()
        };
        for &timestamp in &burst[..reliable_end] {
            let timestamp = timestamp.ok_or("The timing probe omitted a frame timestamp.")?;
            if let Some(before) = previous {
                let delta = timestamp
                    .checked_sub(before)
                    .ok_or("The timing probe returned an invalid timestamp gap.")?;
                if delta <= 0 {
                    return Err("The timing probe returned unordered timestamps.".into());
                }
                deltas.push(delta);
            }
            previous = Some(timestamp);
        }
    }
    if deltas.len() < 80 {
        return Err("The timing probe returned too few frame intervals.".into());
    }

    let shortest = *deltas.iter().min().unwrap();
    let mut quantum = deltas.iter().copied().reduce(gcd).unwrap();
    if quantum == shortest {
        quantum = 1;
    }
    let r_score = r_rate.and_then(|rate| candidate_score(&deltas, rate, time_base, quantum));
    let avg_score = avg_rate.and_then(|rate| candidate_score(&deltas, rate, time_base, quantum));
    let source_rate = match (r_rate, r_score, avg_rate, avg_score) {
        (Some(rate), Some(score), _, average)
            if score >= 0.50 && average.is_none_or(|average| score >= average) =>
        {
            rate
        }
        (_, _, Some(rate), Some(score)) if score >= 0.50 => rate,
        (_, _, Some(rate), _) => rate,
        (Some(rate), _, _, _) => rate,
        _ => return Err("The timing probe could not determine source FPS.".into()),
    };
    let (lower, upper) = allowed_deltas(source_rate, time_base, quantum)
        .ok_or("The source FPS does not fit the video timestamp clock.")?;
    let mut outliers = deltas
        .iter()
        .filter(|delta| **delta != lower && **delta != upper)
        .count();

    let nominal = 1.0 / (source_rate.fps() * time_base.fps());
    let mean = deltas.iter().map(|delta| *delta as f64).sum::<f64>() / deltas.len() as f64;
    let sample_count = timestamps.len().div_ceil(sample_size);
    let tolerance = 0.05_f64.max(quantum as f64 * sample_count as f64 / deltas.len() as f64 + 1e-6);
    if (mean - nominal).abs() > tolerance {
        outliers = outliers.max(1);
    }
    Ok(Timing {
        source_rate,
        variable: outliers > 0,
    })
}

/// Run on the same cancellable worker as the style probe, without seeking playback.
pub fn probe_local_timing(
    ffprobe: &Path,
    video: &Path,
    duration: f64,
    video_stream: u32,
    cancel: &AtomicBool,
) -> Result<Timing, String> {
    if !ffprobe.is_file() || !video.is_file() || !duration.is_finite() || duration <= 0.0 {
        return Err("A local video and bundled FFprobe are required for timing analysis.".into());
    }
    let video_path = video.to_str().ok_or("The video path is not valid UTF-8.")?;
    let selector = video_stream.to_string();
    let intervals = SAMPLE_FRACTIONS
        .into_iter()
        .map(|fraction| format!("{:.3}%+#{FRAMES_PER_SAMPLE}", duration * fraction))
        .collect::<Vec<_>>()
        .join(",");
    let bytes = run_bounded(
        ffprobe,
        &[
            "-v",
            "error",
            "-select_streams",
            &selector,
            "-show_entries",
            "stream=avg_frame_rate,r_frame_rate,time_base",
            "-show_entries",
            "frame=best_effort_timestamp",
            "-read_intervals",
            &intervals,
            "-of",
            "json",
            "--",
            video_path,
        ],
        cancel,
        Duration::from_secs(8),
        MAX_OUTPUT_BYTES,
        None,
    )?;
    let document: TimingDocument = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Parse video timing metadata: {error}"))?;
    analyze(document)
}

pub struct NetworkSource<'a> {
    pub url: &'a str,
    pub video_stream: &'a str,
    pub headers: &'a [String],
    pub proxy: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub referrer: Option<&'a str>,
    pub request_size: Option<u64>,
    pub cookies_in_use: bool,
}

fn probe_url(url: &str) -> Result<String, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("The open video has no HTTP probe URL.".into());
    }
    if let Some(rest) = url.strip_prefix("http://127.0.0.1:") {
        if let Some((port, path)) = rest.split_once('/') {
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(media) = path.strip_prefix("media/") {
                    if let Some((hash, file)) = media.split_once('/') {
                        if !hash.is_empty()
                            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                            && !file.is_empty()
                            && !file.chars().any(|ch| ch == '?' || ch == '#')
                        {
                            return Ok(format!("{url}?role=probe"));
                        }
                    }
                }
            }
        }
    }
    Ok(url.to_owned())
}

pub fn probe_network_timing(
    ffprobe: &Path,
    source: NetworkSource<'_>,
    cancel: &AtomicBool,
) -> Result<Timing, String> {
    if !ffprobe.is_file() {
        return Err("Bundled FFprobe is required for network timing analysis.".into());
    }
    if source.cookies_in_use || !source.headers.is_empty() {
        return Err("The protected stream cannot be probed outside playback.".into());
    }
    let url = probe_url(source.url)?;
    if source.video_stream.parse::<u32>().is_err()
        && !source
            .video_stream
            .strip_prefix("v:")
            .is_some_and(|index| index.parse::<u32>().is_ok())
    {
        return Err("The selected video stream has no FFmpeg selector.".into());
    }

    let timeout = Duration::from_secs(30);
    let timeout_us = timeout.as_micros().to_string();
    let request_size = source.request_size.map(|size| size.to_string());
    let headers = (!source.headers.is_empty()).then(|| source.headers.join("\r\n") + "\r\n");
    let mut args = vec!["-v", "error", "-rw_timeout", &timeout_us];
    for (name, value) in [
        ("-http_proxy", source.proxy),
        ("-user_agent", source.user_agent),
        ("-referer", source.referrer),
        ("-headers", headers.as_deref()),
        ("-request_size", request_size.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            args.extend([name, value]);
        }
    }
    args.extend([
        "-select_streams",
        source.video_stream,
        "-show_entries",
        "stream=avg_frame_rate,r_frame_rate,time_base",
        "-show_entries",
        "packet=pts,dts",
        "-read_intervals",
        "%+#192",
        "-of",
        "json",
        "--",
        &url,
    ]);
    let bytes = run_bounded(ffprobe, &args, cancel, timeout, MAX_OUTPUT_BYTES, None)?;
    let document: TimingDocument = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Parse network timing metadata: {error}"))?;
    analyze(document)
}
