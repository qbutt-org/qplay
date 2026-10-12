use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::Deserialize;

use super::content::{Classification, StyleMetrics, classify};
use super::process::run_bounded;

const SAMPLE_FRACTIONS: [f64; 6] = [0.12, 0.27, 0.42, 0.57, 0.72, 0.87];
const SAMPLE_FRAMES: usize = 12;
const STYLE_FRAMES: [usize; 3] = [0, 4, 8];
const MAX_OUTPUT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct ProbeDocument {
    frames: Vec<ProbeFrame>,
}

#[derive(Deserialize)]
struct ProbeFrame {
    stream_index: usize,
    tags: HashMap<String, String>,
}

impl ProbeFrame {
    fn number(&self, tag: &str) -> Result<f64, String> {
        self.tags
            .get(tag)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("The video probe omitted {tag}."))
    }
}

fn graph_path(path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("The video path is not valid UTF-8.")?;
    if path.chars().any(|ch| matches!(ch, '\0' | '\r' | '\n')) {
        return Err("The video path contains an invalid character.".into());
    }
    let path = path
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "'\\\\\\''");
    Ok(format!("'{path}'"))
}

fn filtergraph(path: &Path, duration: f64, stream: u32) -> Result<String, String> {
    let path = graph_path(path)?;
    let mut filters = Vec::with_capacity(SAMPLE_FRACTIONS.len() * 3);
    for (index, fraction) in SAMPLE_FRACTIONS.into_iter().enumerate() {
        let mut source = String::new();
        write!(
            source,
            "movie=filename={path}:stream_index={stream}:seek_point={:.3}:dec_threads=1,\
             trim=end_frame={SAMPLE_FRAMES},setpts=PTS-STARTPTS,\
             scale=96:54:flags=area,split=2[base{index}][edge{index}]",
            duration * fraction,
        )
        .unwrap();
        filters.push(source);
    }
    for index in 0..SAMPLE_FRACTIONS.len() {
        filters.push(format!("[base{index}]signalstats,entropy[out{index}]"));
    }
    for index in 0..SAMPLE_FRACTIONS.len() {
        filters.push(format!(
            "[edge{index}]edgedetect=low=0.024:high=0.094:mode=wires,\
             signalstats,entropy[out{}]",
            index + SAMPLE_FRACTIONS.len(),
        ));
    }
    Ok(filters.join(";"))
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn analyze(document: ProbeDocument) -> Result<Classification, String> {
    let mut streams: [Vec<ProbeFrame>; 12] = std::array::from_fn(|_| Vec::new());
    for frame in document.frames {
        if let Some(stream) = streams.get_mut(frame.stream_index) {
            stream.push(frame);
        }
    }
    if streams.iter().any(|stream| stream.len() < SAMPLE_FRAMES) {
        return Err("The video probe returned too few sampled frames.".into());
    }

    let mut repeat_pairs = 0;
    let mut repeat_bursts = 0;
    let mut base_entropy = Vec::with_capacity(18);
    let mut saturation = Vec::with_capacity(18);
    let mut edge_average = Vec::with_capacity(18);
    let mut edge_high = Vec::with_capacity(18);
    let mut edge_entropy = Vec::with_capacity(18);
    for index in 0..SAMPLE_FRACTIONS.len() {
        let base = &streams[index];
        let edge = &streams[index + SAMPLE_FRACTIONS.len()];
        let mut burst_repeats = 0;
        for frame in &base[1..SAMPLE_FRAMES] {
            if frame.number("lavfi.signalstats.YDIF")? <= 0.15 {
                repeat_pairs += 1;
                burst_repeats += 1;
            }
        }
        if burst_repeats >= 2 {
            repeat_bursts += 1;
        }
        for frame_index in STYLE_FRAMES {
            base_entropy
                .push(base[frame_index].number("lavfi.entropy.normalized_entropy.normal.Y")?);
            saturation.push(base[frame_index].number("lavfi.signalstats.SATAVG")?);
            edge_average.push(edge[frame_index].number("lavfi.signalstats.YAVG")?);
            edge_high.push(edge[frame_index].number("lavfi.signalstats.YHIGH")?);
            edge_entropy
                .push(edge[frame_index].number("lavfi.entropy.normalized_entropy.normal.Y")?);
        }
    }
    Ok(classify(StyleMetrics {
        repeat_fraction: repeat_pairs as f64
            / (SAMPLE_FRACTIONS.len() * (SAMPLE_FRAMES - 1)) as f64,
        repeat_bursts: repeat_bursts as f64,
        edge_average: mean(&edge_average),
        edge_high: mean(&edge_high),
        edge_entropy: mean(&edge_entropy),
        base_entropy: mean(&base_entropy),
        saturation: mean(&saturation),
    }))
}

/// Run on a worker. The caller cancels the token when the media generation changes.
pub fn classify_file(
    ffprobe: &Path,
    video: &Path,
    duration: f64,
    video_stream: u32,
    cancel: &AtomicBool,
) -> Result<Classification, String> {
    if !ffprobe.is_file() || !video.is_file() || !duration.is_finite() || duration <= 0.0 {
        return Err("A local video and bundled FFprobe are required for style analysis.".into());
    }
    let graph = filtergraph(video, duration, video_stream)?;
    let bytes = run_bounded(
        ffprobe,
        &[
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &graph,
            "-show_entries",
            "frame=stream_index:frame_tags=lavfi.signalstats.YDIF,lavfi.signalstats.SATAVG,\
             lavfi.signalstats.YHIGH,lavfi.signalstats.YAVG,lavfi.entropy.normalized_entropy.normal.Y",
            "-of",
            "json",
        ],
        cancel,
        Duration::from_secs(8),
        MAX_OUTPUT_BYTES,
        None,
    )?;
    let document: ProbeDocument = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Parse video style metadata: {error}"))?;
    analyze(document)
}
