use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde::Deserialize;

use super::ffi;
use crate::gpu::content::{ContentChoice, ContentKind};
use crate::gpu::framegen::{
    self, FramegenChoice, PlaybackState, QualityAction, QualitySample, Rate, Selection,
    SessionQuality,
};
use crate::gpu::hardware::{self, HardwareProfile};
use crate::gpu::probe::classify_file;
use crate::gpu::runtime::{Manager, Runtime};
use crate::gpu::shaders::{self, AutoOptions, ShaderChoice, ShaderPreset, ShaderQuality};
use crate::gpu::timing::{self, Timing};

pub struct GpuState {
    pub shader_choice: ShaderChoice,
    pub content_choice: ContentChoice,
    pub framegen_choice: FramegenChoice,
    pub detected: ContentKind,
    pub applied_shader: ShaderPreset,
    pub framegen_status: String,
    pub error: String,
    pub quality: Option<SessionQuality>,
    pub selection: Option<Selection>,
    pub filter: Option<String>,
    pub active_filter: Option<String>,
    pub pending_manual_shader: bool,
    pub clock: Instant,
    pub last_quality_poll: f64,
    pub interlaced_seen: bool,
    pub video_signature: Option<String>,
    pub runtime: Option<Runtime>,
    pub timing: Option<Timing>,
    pub hardware: Option<HardwareProfile>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub generation: u64,
    pub engine_root: Option<PathBuf>,
    pub runtime_root: Option<PathBuf>,
    pub display_rate: Option<Rate>,
}

impl Default for GpuState {
    fn default() -> Self {
        Self {
            shader_choice: ShaderChoice::Auto,
            content_choice: ContentChoice::Auto,
            framegen_choice: FramegenChoice::Auto,
            detected: ContentKind::Unknown,
            applied_shader: ShaderPreset::Off,
            framegen_status: "off".into(),
            error: String::new(),
            quality: None,
            selection: None,
            filter: None,
            active_filter: None,
            pending_manual_shader: false,
            clock: Instant::now(),
            last_quality_poll: 0.0,
            interlaced_seen: false,
            video_signature: None,
            runtime: None,
            timing: None,
            hardware: None,
            cancel: None,
            generation: 0,
            engine_root: None,
            runtime_root: None,
            display_rate: None,
        }
    }
}

impl GpuState {
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        self.generation += 1;
        self.quality = None;
        self.selection = None;
        self.filter = None;
        self.pending_manual_shader = false;
        self.clock = Instant::now();
        self.last_quality_poll = 0.0;
        self.runtime = None;
        self.timing = None;
        self.detected = ContentKind::Unknown;
        self.framegen_status = "off".into();
        self.error.clear();
    }
}

pub enum GpuCommand {
    Shader {
        preset: ShaderPreset,
        generation: u64,
    },
    AddFilter {
        filter: String,
        generation: u64,
    },
    RemoveFilter {
        apply_shader: bool,
        generation: u64,
    },
}

#[derive(Deserialize)]
struct VideoTrack {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    selected: bool,
    #[serde(rename = "ff-index")]
    ff_index: Option<u32>,
    title: Option<String>,
}

pub(super) fn video_signature(json: &str) -> Option<String> {
    let tracks: Vec<VideoTrack> = serde_json::from_str(json).ok()?;
    tracks
        .into_iter()
        .find(|track| track.kind == "video" && track.selected)
        .map(|track| {
            format!(
                "{}:{}",
                track.ff_index.unwrap_or(0),
                track.title.unwrap_or_default()
            )
        })
}

struct ProbeInput {
    ffprobe: PathBuf,
    local: Option<PathBuf>,
    duration: f64,
    stream: u32,
    stream_title: String,
    stream_filename: Option<String>,
    ytdl_result: Option<String>,
    http_headers: Vec<String>,
    http_proxy: Option<String>,
    user_agent: Option<String>,
    referrer: Option<String>,
    request_size: Option<u64>,
    cookies_in_use: bool,
    display_rate: Option<Rate>,
    width: u32,
    height: u32,
    interlaced: bool,
    matrix: String,
    full_range: bool,
    framegen_choice: FramegenChoice,
    shader_choice: ShaderChoice,
    content_choice: ContentChoice,
    engine_root: PathBuf,
    runtime_root: PathBuf,
}

struct ProbeResult {
    detected: ContentKind,
    timing: Option<Timing>,
    hardware: Option<HardwareProfile>,
    runtime: Option<Runtime>,
    selection: Option<Selection>,
    filter: Option<String>,
    framegen_status: String,
    error: Option<String>,
}

fn probe(input: ProbeInput, cancel: &AtomicBool, on_prepare: impl FnOnce()) -> ProbeResult {
    let detected = input
        .local
        .as_ref()
        .and_then(|path| {
            classify_file(&input.ffprobe, path, input.duration, input.stream, cancel)
                .ok()
                .filter(|classification| classification.confidence >= 0.55)
                .map(|classification| classification.kind)
        })
        .unwrap_or(ContentKind::Unknown);
    let timing_result = if let Some(path) = input.local.as_ref() {
        timing::probe_local_timing(&input.ffprobe, path, input.duration, input.stream, cancel)
    } else if let Some(filename) = input.stream_filename.as_deref() {
        timing::resolve_network_source(
            filename,
            &input.stream_title,
            input.stream,
            input.ytdl_result.as_deref(),
        )
        .and_then(|resolved| {
            timing::probe_network_timing(
                &input.ffprobe,
                timing::NetworkSource {
                    url: &resolved.url,
                    video_stream: &resolved.video_stream,
                    headers: &input.http_headers,
                    proxy: input.http_proxy.as_deref(),
                    user_agent: input.user_agent.as_deref(),
                    referrer: input.referrer.as_deref(),
                    request_size: input.request_size,
                    cookies_in_use: input.cookies_in_use,
                },
                cancel,
            )
        })
    } else {
        Err("The selected video source is unavailable for FPS analysis.".into())
    };
    let timing = timing_result.as_ref().ok().copied();
    let hardware = hardware::detect().ok();
    let wants_framegen = !matches!(input.shader_choice,
        ShaderChoice::Manual(preset) if preset != ShaderPreset::Fsr)
        && match input.framegen_choice {
            FramegenChoice::Off => false,
            FramegenChoice::On => true,
            FramegenChoice::Auto => input.content_choice.effective(detected) == ContentKind::Movie,
        };
    let guard = if input.interlaced {
        Some("Frame generation is unavailable for interlaced video.".to_owned())
    } else {
        match (timing_result.as_ref(), input.display_rate) {
            (Err(error), _) => Some(error.clone()),
            (Ok(timing), _) if timing.variable => {
                Some("Frame generation needs constant video frame timing.".to_owned())
            }
            (Ok(_), None) => Some("The display refresh rate is unavailable.".to_owned()),
            (Ok(timing), Some(display)) if display.fps() <= timing.source_rate.fps() => {
                Some("The display has no frame-rate headroom for this video.".to_owned())
            }
            _ => None,
        }
    };
    let (runtime, mut error) = if wants_framegen && guard.is_none() {
        on_prepare();
        let manager = Manager::new(&input.engine_root, &input.runtime_root);
        match manager.prepare(cancel) {
            Ok(runtime) => (Some(runtime), None),
            Err(error) => (None, Some(error)),
        }
    } else {
        let error = (input.framegen_choice == FramegenChoice::On)
            .then_some(guard)
            .flatten();
        (None, error)
    };
    let mut filter = None;
    let mut selection = None;
    let mut framegen_status = if wants_framegen { "guarded" } else { "off" }.to_owned();
    if let (Some(runtime), Some(timing)) = (&runtime, timing) {
        let video = framegen::VideoInfo {
            has_video: true,
            width: input.width,
            height: input.height,
            source_rate: Some(timing.source_rate),
            display_rate: input.display_rate,
            timing_verified: !timing.variable,
            interlaced: input.interlaced,
            content_kind: input.content_choice.effective(detected),
        };
        match framegen::choose(
            input.framegen_choice,
            video,
            Some(runtime.backend()),
            runtime.model_path().is_file(),
        ) {
            Ok(Some(chosen)) => {
                let script = input.engine_root.join("profile/framegen/framegen.vpy");
                let preparer = input.engine_root.join("profile/framegen/prepare.py");
                let manager = Manager::new(&input.engine_root, &input.runtime_root);
                match framegen::graph(
                    runtime,
                    &script,
                    timing.source_rate,
                    chosen,
                    &input.matrix,
                    input.full_range,
                )
                .and_then(|(spec, config)| {
                    let ready = runtime.prepare_engine(&preparer, &config, cancel)?;
                    if ready {
                        runtime.load_vsscript(manager.runtime_root())?;
                    }
                    Ok((spec, ready))
                }) {
                    Ok((spec, true)) => {
                        selection = Some(chosen);
                        filter = Some(spec);
                        framegen_status = "starting".into();
                    }
                    Ok((_, false)) => framegen_status = "prepared".into(),
                    Err(reason) => {
                        error = Some(reason);
                        framegen_status = "error".into();
                    }
                }
            }
            Ok(None) => framegen_status = "off".into(),
            Err(_) => framegen_status = "guarded".into(),
        }
    } else if error.is_some() {
        framegen_status = "unavailable".into();
    }
    ProbeResult {
        detected,
        timing,
        hardware,
        runtime,
        selection,
        filter,
        framegen_status,
        error,
    }
}

impl ffi::PlayerController {
    pub(super) fn set_display_rate(mut self: Pin<&mut Self>, fps: f64) {
        let rate = Rate::from_fps(fps);
        if self.gpu.display_rate == rate {
            return;
        }
        self.as_mut().rust_mut().gpu.display_rate = rate;
        if self.file_loaded {
            self.as_mut().begin_gpu_probe();
        }
    }

    pub(super) fn cancel_gpu(mut self: Pin<&mut Self>) {
        let generation = self.gpu.generation;
        if self.gpu.active_filter.is_some() || self.gpu.filter.is_some() {
            self.as_mut().dispatch_gpu(
                &["vf", "remove", "@mpv-framegen"],
                GpuCommand::RemoveFilter {
                    apply_shader: false,
                    generation,
                },
            );
            self.as_mut().rust_mut().gpu.active_filter = None;
        }
        self.as_mut().rust_mut().gpu.cancel();
    }

    pub(super) fn begin_gpu_probe(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_gpu();
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(engine_root) = self.gpu.engine_root.clone() else {
            return;
        };
        let Some(runtime_root) = self.gpu.runtime_root.clone() else {
            return;
        };
        let Some(media) = self.media.clone() else {
            return;
        };
        let tracks: Vec<VideoTrack> = session
            .text("track-list")
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let Some(video) = tracks
            .into_iter()
            .find(|track| track.kind == "video" && track.selected)
        else {
            return;
        };
        let stream = video.ff_index.unwrap_or(0);
        let title = video.title.unwrap_or_default();
        self.as_mut().rust_mut().gpu.video_signature = Some(format!("{stream}:{title}"));
        let local = (!media.contains("://"))
            .then(|| PathBuf::from(media))
            .filter(|path| path.is_file());
        let input = ProbeInput {
            ffprobe: engine_root.join("ffprobe.exe"),
            local,
            duration: session.number("duration").unwrap_or(self.duration),
            stream,
            stream_title: title,
            stream_filename: session.text("stream-open-filename"),
            ytdl_result: session.text("user-data/mpv/ytdl/json-subprocess-result"),
            http_headers: session
                .text("http-header-fields")
                .and_then(|value| serde_json::from_str(&value).ok())
                .unwrap_or_default(),
            http_proxy: session.text("http-proxy"),
            user_agent: session.text("user-agent"),
            referrer: session.text("referrer"),
            request_size: session
                .number("stream-lavf-o/request_size")
                .map(|value| value as u64),
            cookies_in_use: session
                .text("cookies-file")
                .is_some_and(|value| !value.is_empty()),
            display_rate: self.gpu.display_rate,
            width: session.number("video-params/w").unwrap_or(0.0) as u32,
            height: session.number("video-params/h").unwrap_or(0.0) as u32,
            interlaced: self.gpu.interlaced_seen,
            matrix: session.text("video-params/colormatrix").unwrap_or_default(),
            full_range: session.text("video-params/colorlevels").as_deref() == Some("full"),
            framegen_choice: self.gpu.framegen_choice,
            shader_choice: self.gpu.shader_choice,
            content_choice: self.gpu.content_choice,
            engine_root,
            runtime_root,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let generation = self.gpu.generation;
        let thread = self.qt_thread();
        self.as_mut().rust_mut().gpu.cancel = Some(cancel.clone());
        std::thread::spawn(move || {
            let progress_thread = thread.clone();
            let result = probe(input, &cancel, || {
                let _ = progress_thread.queue(move |mut player| {
                    if player.gpu.generation == generation {
                        player.as_mut().rust_mut().gpu.framegen_status = "loading".into();
                        player.as_mut().state_changed();
                    }
                });
            });
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let _ = thread.queue(move |mut player| {
                if player.gpu.generation != generation || !player.file_loaded {
                    return;
                }
                {
                    let gpu = &mut player.as_mut().rust_mut().gpu;
                    gpu.detected = result.detected;
                    gpu.timing = result.timing;
                    gpu.hardware = result.hardware;
                    gpu.runtime = result.runtime;
                    gpu.selection = result.selection;
                    gpu.filter = result.filter;
                    gpu.framegen_status = result.framegen_status;
                    gpu.error = result.error.unwrap_or_default();
                    gpu.cancel = None;
                }
                player.as_mut().apply_gpu_policy();
            });
        });
    }

    pub(super) fn apply_gpu_policy(mut self: Pin<&mut Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let generation = self.gpu.generation;
        let manual_non_fsr = matches!(self.gpu.shader_choice,
            ShaderChoice::Manual(preset) if preset != ShaderPreset::Fsr);
        if manual_non_fsr {
            if self.gpu.pending_manual_shader {
                return;
            }
            if self.gpu.active_filter.is_some() || self.gpu.filter.is_some() {
                self.as_mut().rust_mut().gpu.pending_manual_shader = true;
                self.as_mut().dispatch_gpu(
                    &["vf", "remove", "@mpv-framegen"],
                    GpuCommand::RemoveFilter {
                        apply_shader: true,
                        generation,
                    },
                );
                return;
            }
            self.as_mut().rust_mut().gpu.framegen_status = "off".into();
        } else if let Some(filter) = self.gpu.filter.clone() {
            if self.gpu.active_filter.as_ref() != Some(&filter) {
                self.as_mut().dispatch_gpu(
                    &["vf", "add", &filter],
                    GpuCommand::AddFilter {
                        filter: filter.clone(),
                        generation,
                    },
                );
            }
        }
        let gpu = &self.gpu;
        let width = session.number("video-params/w").unwrap_or(0.0) as u32;
        let height = session.number("video-params/h").unwrap_or(0.0) as u32;
        let has_video = session.text("vid").is_some_and(|vid| vid != "no");
        let kind = gpu.content_choice.effective(gpu.detected);
        let hardware = gpu.hardware.as_ref();
        let choice = shaders::choose_preset(
            gpu.shader_choice,
            shaders::VideoInfo {
                has_video,
                width,
                height,
                fps: session.number("container-fps").unwrap_or(0.0),
                display_width: session.number("osd-width").unwrap_or(0.0) as u32,
                display_height: session.number("osd-height").unwrap_or(0.0) as u32,
                video_bitrate: session.number("video-bitrate").unwrap_or(0.0),
                file_size: session.number("file-size").unwrap_or(0.0),
                duration: self.duration,
                content_kind: kind,
            },
            self.gpu.filter.is_some() && !manual_non_fsr,
            AutoOptions {
                max_gan_scale: hardware.map_or(0, |profile| profile.max_gan_scale),
            },
        );
        let quality = hardware.map_or(ShaderQuality::Fast, |profile| profile.quality);
        let Some(shader_dir) = gpu
            .engine_root
            .as_ref()
            .map(|root| root.join("profile/shaders"))
        else {
            return;
        };
        match choice.mpv_value(quality, &shader_dir) {
            Ok(value) => {
                self.as_mut().dispatch_gpu(
                    &["set", "glsl-shaders", &value],
                    GpuCommand::Shader {
                        preset: choice,
                        generation,
                    },
                );
            }
            Err(error) => self.as_mut().rust_mut().gpu.error = error,
        }
        self.as_mut().state_changed();
    }

    pub(super) fn dispatch_gpu(mut self: Pin<&mut Self>, arguments: &[&str], action: GpuCommand) {
        let Some(session) = self.session.clone() else {
            return;
        };
        match session.command(arguments) {
            Ok(id) => {
                self.as_mut().rust_mut().gpu_commands.insert(id, action);
            }
            Err(error) => {
                let gpu = &mut self.as_mut().rust_mut().gpu;
                if matches!(
                    action,
                    GpuCommand::RemoveFilter {
                        apply_shader: true,
                        ..
                    }
                ) {
                    gpu.pending_manual_shader = false;
                }
                gpu.error = error;
                self.as_mut().state_changed();
            }
        }
    }

    pub(super) fn gpu_command_complete(mut self: Pin<&mut Self>, action: GpuCommand, error: i32) {
        let relevant = match &action {
            GpuCommand::Shader { generation, .. } | GpuCommand::AddFilter { generation, .. } => {
                *generation == self.gpu.generation
            }
            GpuCommand::RemoveFilter {
                apply_shader,
                generation,
            } => *apply_shader && *generation == self.gpu.generation,
        };
        if error < 0 {
            if !relevant {
                return;
            }
            let message = self
                .session
                .as_ref()
                .map(|session| session.error(error))
                .unwrap_or_default();
            self.as_mut().rust_mut().gpu.error = message;
            if !matches!(&action, GpuCommand::Shader { .. }) {
                self.as_mut().rust_mut().gpu.framegen_status = "error".into();
            }
            self.as_mut().rust_mut().gpu.pending_manual_shader = false;
            return;
        }
        match action {
            GpuCommand::Shader { preset, generation } if generation == self.gpu.generation => {
                let gpu = &mut self.as_mut().rust_mut().gpu;
                gpu.applied_shader = preset;
                gpu.error.clear();
            }
            GpuCommand::AddFilter { filter, generation } if generation == self.gpu.generation => {
                self.as_mut().rust_mut().gpu.active_filter = Some(filter);
                self.as_mut().rust_mut().gpu.framegen_status = "starting".into();
                self.as_mut().rust_mut().gpu.error.clear();
                if let (Some(timing), Some(selection), Some(runtime)) = (
                    self.gpu.timing,
                    self.gpu.selection,
                    self.gpu.runtime.as_ref(),
                ) {
                    let display = self.gpu.display_rate;
                    if let Some(display) = display {
                        let ladder =
                            framegen::target_ladder(timing.source_rate, display, runtime.backend());
                        self.as_mut().rust_mut().gpu.quality =
                            SessionQuality::new(ladder, selection.target.rate);
                    }
                }
            }
            GpuCommand::RemoveFilter {
                apply_shader,
                generation,
            } => {
                if apply_shader && generation == self.gpu.generation {
                    let gpu = &mut self.as_mut().rust_mut().gpu;
                    gpu.active_filter = None;
                    gpu.filter = None;
                    gpu.quality = None;
                    gpu.pending_manual_shader = false;
                    gpu.framegen_status = "off".into();
                    self.as_mut().apply_gpu_policy();
                }
            }
            _ => {}
        }
    }

    pub(super) fn tick_gpu_quality(mut self: Pin<&mut Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let generation = self.gpu.generation;
        let Some(selection) = self.gpu.selection else {
            return;
        };
        if (self.speed - 1.0).abs() > 0.001 {
            return;
        }
        let present = session
            .text("vf")
            .is_some_and(|filters| filters.contains("mpv-framegen"));
        if self.gpu.active_filter.is_some() && !present {
            let gpu = &mut self.as_mut().rust_mut().gpu;
            gpu.active_filter = None;
            gpu.filter = None;
            gpu.quality = None;
            gpu.framegen_status = "error".into();
            gpu.error = "The frame generation filter stopped.".into();
            self.as_mut().apply_gpu_policy();
            return;
        }
        let now = self.gpu.clock.elapsed().as_secs_f64();
        if now - self.gpu.last_quality_poll < 0.5 {
            return;
        }
        self.as_mut().rust_mut().gpu.last_quality_poll = now;
        let count = |name: &str| session.number(name).unwrap_or(0.0).max(0.0) as u64;
        let sample = QualitySample {
            playback_position: session.number("time-pos").unwrap_or(0.0),
            filter_fps: session.number("estimated-vf-fps").unwrap_or(0.0),
            frame_drops: count("frame-drop-count"),
            decoder_drops: count("decoder-frame-drop-count"),
            mistimed_frames: count("mistimed-frame-count"),
            delayed_frames: count("vo-delayed-frame-count"),
        };
        let state = PlaybackState {
            now,
            paused: self.paused,
            seeking: session.flag("seeking").unwrap_or(false),
            caching: session.flag("paused-for-cache").unwrap_or(false),
            eof: session.flag("eof-reached").unwrap_or(false),
            idle: session.flag("core-idle").unwrap_or(false),
            filter_active: self.gpu.active_filter.is_some() && present,
            output_ready: session.number("video-out-params/w")
                == Some(selection.output_width as f64)
                && session.number("video-out-params/h") == Some(selection.output_height as f64),
            sample,
        };
        if self.gpu.framegen_status == "starting"
            && state.filter_active
            && state.output_ready
            && state.sample.playback_position > 0.0
            && (state.sample.filter_fps - selection.target.rate.fps()).abs()
                <= selection.target.rate.fps() * 0.03
        {
            self.as_mut().rust_mut().gpu.framegen_status = "active".into();
            self.as_mut().state_changed();
        }
        let action = self
            .as_mut()
            .rust_mut()
            .gpu
            .quality
            .as_mut()
            .map(|quality| quality.tick(state))
            .unwrap_or(QualityAction::Keep);
        match action {
            QualityAction::Keep => {}
            QualityAction::Lower => self.as_mut().lower_framegen(),
            QualityAction::DisableUntilNextVideo => {
                self.as_mut().dispatch_gpu(
                    &["vf", "remove", "@mpv-framegen"],
                    GpuCommand::RemoveFilter {
                        apply_shader: false,
                        generation,
                    },
                );
                let gpu = &mut self.as_mut().rust_mut().gpu;
                gpu.active_filter = None;
                gpu.filter = None;
                gpu.framegen_status = "guarded".into();
            }
        }
    }

    fn lower_framegen(mut self: Pin<&mut Self>) {
        let generation = self.gpu.generation;
        let (Some(runtime), Some(timing), Some(mut selection), Some(engine_root)) = (
            self.gpu.runtime.as_ref(),
            self.gpu.timing,
            self.gpu.selection,
            self.gpu.engine_root.as_ref(),
        ) else {
            return;
        };
        selection.target = self.gpu.quality.as_ref().unwrap().target();
        let Some(session) = self.session.clone() else {
            return;
        };
        let matrix = session.text("video-params/colormatrix").unwrap_or_default();
        let full_range = session.text("video-params/colorlevels").as_deref() == Some("full");
        let script = engine_root.join("profile/framegen/framegen.vpy");
        match framegen::graph(
            runtime,
            &script,
            timing.source_rate,
            selection,
            &matrix,
            full_range,
        ) {
            Ok((filter, _)) => {
                self.as_mut().dispatch_gpu(
                    &["vf", "remove", "@mpv-framegen"],
                    GpuCommand::RemoveFilter {
                        apply_shader: false,
                        generation,
                    },
                );
                self.as_mut().dispatch_gpu(
                    &["vf", "add", &filter],
                    GpuCommand::AddFilter {
                        filter: filter.clone(),
                        generation,
                    },
                );
                let gpu = &mut self.as_mut().rust_mut().gpu;
                gpu.selection = Some(selection);
                gpu.filter = Some(filter);
                gpu.active_filter = None;
                gpu.framegen_status = "starting".into();
            }
            Err(error) => self.as_mut().rust_mut().gpu.error = error,
        }
    }

    pub(super) fn change_shader(mut self: Pin<&mut Self>, choice: &QString) {
        let Some(choice) = ShaderChoice::from_id(&choice.to_string()) else {
            return;
        };
        let manual_non_fsr =
            matches!(choice, ShaderChoice::Manual(preset) if preset != ShaderPreset::Fsr);
        let had_filter = self.gpu.active_filter.is_some() || self.gpu.filter.is_some();
        if manual_non_fsr {
            self.as_mut().rust_mut().gpu.cancel();
        }
        self.as_mut().rust_mut().gpu.shader_choice = choice;
        self.as_mut().save_preferences();
        if self.media_loaded() {
            if manual_non_fsr {
                if had_filter {
                    let generation = self.gpu.generation;
                    self.as_mut().rust_mut().gpu.pending_manual_shader = true;
                    self.as_mut().dispatch_gpu(
                        &["vf", "remove", "@mpv-framegen"],
                        GpuCommand::RemoveFilter {
                            apply_shader: true,
                            generation,
                        },
                    );
                } else {
                    self.as_mut().apply_gpu_policy();
                }
            } else {
                self.as_mut().begin_gpu_probe();
            }
        }
    }

    pub(super) fn change_content(mut self: Pin<&mut Self>, choice: &QString) {
        let Some(choice) = ContentChoice::from_id(&choice.to_string()) else {
            return;
        };
        self.as_mut().rust_mut().gpu.content_choice = choice;
        self.as_mut().save_preferences();
        if self.media_loaded() {
            self.as_mut().begin_gpu_probe();
        }
    }

    pub(super) fn change_framegen(mut self: Pin<&mut Self>, choice: &QString) {
        let Some(choice) = FramegenChoice::from_id(&choice.to_string()) else {
            return;
        };
        self.as_mut().rust_mut().gpu.framegen_choice = choice;
        self.as_mut().save_preferences();
        if self.media_loaded() {
            self.as_mut().begin_gpu_probe();
        }
    }
}
