use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList};
use qbutt_codex::{self as codex, CodexModel, ModelModality};
use tokio::sync::watch;

use super::ffi;
use crate::preferences::SubtitleTranslationPreferences;
use crate::subtitles::{
    MpvEmbeddedTrack, PlaybackMedia, SubtitleFormat, SubtitleIdentity, SubtitleService,
    SubtitleSourceError, SubtitleTranslationOptions, VerifiedMediaTools, extract_local_embedded,
    extract_network_matroska, verify_embedded_track, window_bounds,
};
use crate::track_model::Track;

pub struct SubtitleTask {
    thread: Box<cxx_qt::CxxQtThread<ffi::PlayerController>>,
    operation: Operation,
}

enum Operation {
    Models(watch::Receiver<bool>),
    Window(WindowTask),
}

struct WindowTask {
    service: Arc<SubtitleService>,
    generation: u64,
    media: Arc<PlaybackMedia>,
    library: PathBuf,
    track: Track,
    demuxer: String,
    origin_ms: i64,
    position_ms: i64,
    preferences: SubtitleTranslationPreferences,
    cancelled: watch::Receiver<bool>,
}

#[expect(
    clippy::boxed_local,
    reason = "CXX transfers this opaque worker task in an owning Rust box"
)]
pub fn run_subtitle_task(task: Box<SubtitleTask>) {
    let thread = task.thread;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    match task.operation {
        Operation::Models(mut cancelled) => {
            let result = match runtime {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = codex::available_models(ModelModality::Text) => result,
                        _ = cancelled.changed() => Err(codex::CodexError::Cancelled),
                    }
                }),
                Err(_) => Err(codex::CodexError::Unavailable),
            };
            if *cancelled.borrow() {
                return;
            }
            let _ = thread.queue(move |mut player| {
                let mut state = player.as_mut().rust_mut();
                state.subtitle_catalog_requested = false;
                state.subtitle_catalog_cancel = None;
                match result {
                    Ok(models) => state.subtitle_catalog = models,
                    Err(_) => state.subtitle_status = "unavailable".into(),
                }
                player.as_mut().state_changed();
            });
        }
        Operation::Window(window) => {
            let generation = window.generation;
            let result = match runtime {
                Ok(runtime) => runtime.block_on(translate_window(window, &thread)),
                Err(_) => Err("unavailable"),
            };
            let _ = thread.queue(move |mut player| {
                if player.subtitle_task_generation != generation {
                    return;
                }
                player.as_mut().rust_mut().subtitle_task_cancel = None;
                if let Err(status) = result {
                    let mut state = player.as_mut().rust_mut();
                    state.subtitle_status = QString::from(status);
                    if state.subtitle_output_path.is_none() {
                        state.subtitle_translation_view = false;
                        state.subtitle_window_start_ms = None;
                        player.as_mut().restore_original_subtitles();
                    }
                } else {
                    if player.subtitle_status.to_string() != "failed" {
                        player.as_mut().rust_mut().subtitle_status = "ready".into();
                    }
                }
                player.as_mut().state_changed();
            });
        }
    }
}

async fn translate_window(
    task: WindowTask,
    thread: &cxx_qt::CxxQtThread<ffi::PlayerController>,
) -> Result<(), &'static str> {
    let digest = task
        .media
        .get(task.cancelled.clone())
        .await
        .map_err(source_status)?;
    let (document, identity) = if task.track.external {
        let source = task.track.external_filename.as_ref().ok_or("unsupported")?;
        let format = external_format(source).ok_or("unsupported")?;
        let (document, source_digest) = task
            .media
            .read_external(source, format, task.cancelled.clone())
            .await
            .map_err(source_status)?;
        let identity = SubtitleIdentity {
            media_digest: digest,
            stream_index: None,
            source_digest,
        };
        (document, identity)
    } else {
        let selected = MpvEmbeddedTrack {
            mpv_id: task.track.id,
            ff_index: task.track.ff_index,
            src_id: task.track.src_id,
            codec: task.track.codec,
            current_demuxer: task.demuxer,
            origin_ms: task.origin_ms,
        };
        let (start, end) = window_bounds(task.position_ms).map_err(|_| "unsupported")?;
        if let Some(media) = task.media.path() {
            let tools = VerifiedMediaTools::from_library(&task.library).map_err(source_status)?;
            let track = verify_embedded_track(media, &tools, &selected, task.cancelled.clone())
                .await
                .map_err(source_status)?;
            let document =
                extract_local_embedded(media, &tools, &track, start, end, task.cancelled.clone())
                    .await
                    .map_err(source_status)?;
            (document, track.identity(digest))
        } else {
            extract_network_matroska(
                task.media.clone(),
                selected,
                start,
                end,
                digest,
                task.cancelled.clone(),
            )
            .await
            .map_err(source_status)?
        }
    };
    let options = task
        .service
        .cached_options(
            &identity,
            &task.preferences.target_language,
            task.preferences.model.as_deref(),
            task.preferences.effort.as_deref(),
            task.generation,
        )
        .map_err(|_| "failed")?;
    let mut accepted = match &options {
        Some(options) => task
            .service
            .cached_window(&document, task.position_ms, options, task.generation)
            .map_err(|_| "failed")?,
        None => HashMap::new(),
    };
    let result = async {
        let options = if let Some(options) = options {
            options
        } else {
            let mut selection_cancel = task.cancelled.clone();
            let (model, effort) = tokio::select! {
                result = codex::resolve_selection(
                    ModelModality::Text,
                    task.preferences.model.as_deref(),
                    task.preferences.effort.as_deref(),
                ) => result.map_err(codex_status)?,
                _ = selection_cancel.changed() => return Err("cancelled"),
            };
            let options = SubtitleTranslationOptions {
                identity,
                target_language: task.preferences.target_language.clone(),
                model,
                effort,
            };
            accepted = task
                .service
                .cached_window(&document, task.position_ms, &options, task.generation)
                .map_err(|_| "failed")?;
            options
        };
        loop {
            if *task.cancelled.borrow() {
                return Err("cancelled");
            }
            let Some(batch) = task
                .service
                .translate_window(
                    &document,
                    task.position_ms,
                    &options,
                    task.generation,
                    task.cancelled.clone(),
                )
                .await
                .map_err(|error| match error {
                    crate::subtitles::SubtitleServiceError::Codex(error) => codex_status(error),
                    _ => "failed",
                })?
            else {
                break;
            };
            let accepted_key = batch.accepted.first().ok_or("failed")?.0.clone();
            for (key, text) in batch.accepted {
                accepted.insert(key.fragment_id, text);
            }
            if task.preferences.model.is_none() || task.preferences.effort.is_none() {
                task.service
                    .remember_selection(
                        &accepted_key,
                        task.preferences.model.as_deref(),
                        task.preferences.effort.as_deref(),
                        task.generation,
                    )
                    .map_err(|_| "failed")?;
            }
        }
        Ok(())
    }
    .await;
    if !accepted.is_empty() {
        let format = document.format;
        let output = task.service.output_path(task.generation, format);
        task.service
            .publish_window(&document, &accepted, &output, task.generation)
            .map_err(|_| "failed")?;
        let generation = task.generation;
        let _ = thread.queue(move |mut player| {
            if player.subtitle_task_generation == generation && player.subtitle_translation_view {
                player.as_mut().present_subtitle_output(&output, format);
            }
        });
    }
    result
}

fn external_format(source: &str) -> Option<SubtitleFormat> {
    let url = reqwest::Url::parse(source).ok();
    let path = Path::new(url.as_ref().map_or(source, |url| url.path()));
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "ass" => Some(SubtitleFormat::Ass),
        "ssa" => Some(SubtitleFormat::Ssa),
        "srt" => Some(SubtitleFormat::Srt),
        "vtt" => Some(SubtitleFormat::Vtt),
        _ => None,
    }
}

fn source_status(error: SubtitleSourceError) -> &'static str {
    match error {
        SubtitleSourceError::Bitmap => "bitmap",
        SubtitleSourceError::Cancelled => "cancelled",
        SubtitleSourceError::TrackMapping | SubtitleSourceError::Source => "unsupported",
        _ => "failed",
    }
}

fn codex_status(error: codex::CodexError) -> &'static str {
    match error {
        codex::CodexError::Cancelled => "cancelled",
        codex::CodexError::Unavailable | codex::CodexError::Authentication => "unavailable",
        codex::CodexError::ModelUnavailable => "modelUnavailable",
        _ => "failed",
    }
}

impl ffi::PlayerController {
    fn dispatch_subtitle(mut self: Pin<&mut Self>, arguments: &[&str]) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.subtitle_commands.len() >= 8 {
            self.as_mut().rust_mut().subtitle_status = "failed".into();
            return;
        }
        match session.command(arguments) {
            Ok(id) => {
                let generation = self.subtitle_task_generation;
                self.as_mut()
                    .rust_mut()
                    .subtitle_commands
                    .insert(id, generation);
            }
            Err(_) => self.as_mut().rust_mut().subtitle_status = "failed".into(),
        }
    }

    pub(super) fn subtitle_translation_enabled(&self) -> bool {
        self.subtitle_translation.enabled
    }

    pub(super) fn subtitle_translation_active(&self) -> bool {
        self.subtitle_translation_view
    }

    pub(super) fn subtitle_target(&self) -> QString {
        QString::from(self.subtitle_translation.target_language.as_str())
    }

    pub(super) fn subtitle_model(&self) -> QString {
        QString::from(self.subtitle_translation.model.as_deref().unwrap_or(""))
    }

    pub(super) fn subtitle_effort(&self) -> QString {
        QString::from(self.subtitle_translation.effort.as_deref().unwrap_or(""))
    }

    pub(super) fn subtitle_models(&self) -> QStringList {
        self.subtitle_catalog
            .iter()
            .map(|model| QString::from(model.id.as_str()))
            .collect()
    }

    pub(super) fn subtitle_efforts(&self) -> QStringList {
        self.selected_model()
            .map(|model| {
                model
                    .efforts
                    .iter()
                    .map(|effort| QString::from(effort.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn subtitle_status(&self) -> QString {
        self.subtitle_status.clone()
    }

    fn selected_model(&self) -> Option<&CodexModel> {
        let chosen = self.subtitle_translation.model.as_deref();
        self.subtitle_catalog
            .iter()
            .find(|model| chosen.map_or(model.is_default, |chosen| model.id == chosen))
    }

    pub(super) fn load_subtitle_models(mut self: Pin<&mut Self>) {
        if self.subtitle_catalog_requested || !self.subtitle_catalog.is_empty() {
            return;
        }
        let (cancel, cancelled) = watch::channel(false);
        let mut state = self.as_mut().rust_mut();
        state.subtitle_catalog_requested = true;
        state.subtitle_catalog_cancel = Some(cancel);
        ffi::start_subtitle_task(Box::new(SubtitleTask {
            thread: Box::new(self.qt_thread()),
            operation: Operation::Models(cancelled),
        }));
    }

    pub(super) fn change_subtitle_translation(mut self: Pin<&mut Self>, enabled: bool) {
        if self.subtitle_translation.enabled == enabled {
            return;
        }
        self.as_mut().rust_mut().subtitle_translation.enabled = enabled;
        self.as_mut().save_preferences();
        if enabled {
            self.as_mut().rust_mut().subtitle_translation_view = true;
            self.as_mut().maybe_start_subtitle_window();
        } else {
            self.as_mut().cancel_subtitle_job();
            self.as_mut().rust_mut().subtitle_translation_view = false;
            self.as_mut().rust_mut().subtitle_status = "idle".into();
            self.as_mut().restore_original_subtitles();
        }
        self.as_mut().state_changed();
    }

    pub(super) fn change_subtitle_target(mut self: Pin<&mut Self>, target: &QString) {
        let target = target.to_string();
        if !matches!(target.as_str(), "ru" | "en")
            || target == self.subtitle_translation.target_language
        {
            return;
        }
        self.as_mut()
            .rust_mut()
            .subtitle_translation
            .target_language = target;
        self.as_mut().save_preferences();
        self.as_mut().restart_subtitle_window();
    }

    pub(super) fn change_subtitle_model(mut self: Pin<&mut Self>, model: &QString) {
        let model = model.to_string();
        if !model.is_empty() && !self.subtitle_catalog.iter().any(|item| item.id == model) {
            return;
        }
        let model = if model.is_empty() { None } else { Some(model) };
        if self.subtitle_translation.model == model {
            return;
        }
        let mut state = self.as_mut().rust_mut();
        state.subtitle_translation.model = model;
        state.subtitle_translation.effort = None;
        self.as_mut().save_preferences();
        self.as_mut().restart_subtitle_window();
    }

    pub(super) fn change_subtitle_effort(mut self: Pin<&mut Self>, effort: &QString) {
        let effort = effort.to_string();
        if !effort.is_empty()
            && !self
                .selected_model()
                .is_some_and(|model| model.efforts.contains(&effort))
        {
            return;
        }
        let effort = if effort.is_empty() {
            None
        } else {
            Some(effort)
        };
        if self.subtitle_translation.effort == effort {
            return;
        }
        self.as_mut().rust_mut().subtitle_translation.effort = effort;
        self.as_mut().save_preferences();
        self.as_mut().restart_subtitle_window();
    }

    pub(super) fn use_translated_subtitles(mut self: Pin<&mut Self>) {
        if !self.media_loaded() {
            return;
        }
        if self.subtitle_original_id.is_none() {
            self.as_mut().rust_mut().subtitle_status = "unsupported".into();
            self.as_mut().state_changed();
            return;
        }
        if !self.subtitle_translation.enabled {
            self.as_mut().change_subtitle_translation(true);
        } else {
            let retry = matches!(
                self.subtitle_status.to_string().as_str(),
                "failed" | "unavailable" | "modelUnavailable"
            );
            self.as_mut().rust_mut().subtitle_translation_view = true;
            if retry {
                self.as_mut().restart_subtitle_window();
            } else {
                self.as_mut().maybe_start_subtitle_window();
            }
        }
        self.as_mut().state_changed();
    }

    pub(super) fn cancel_subtitle_job(mut self: Pin<&mut Self>) {
        if let Some(service) = &self.subtitle_service {
            let generation = service.begin_generation();
            self.as_mut().rust_mut().subtitle_task_generation = generation;
        }
        if let Some(cancel) = self.as_mut().rust_mut().subtitle_task_cancel.take() {
            let _ = cancel.send(true);
        }
        self.as_mut().rust_mut().subtitle_commands.clear();
        self.as_mut().rust_mut().subtitle_window_start_ms = None;
    }

    pub(super) fn restart_subtitle_window(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_subtitle_job();
        self.as_mut().restore_original_subtitles();
        if self.subtitle_translation.enabled && self.subtitle_translation_view {
            self.as_mut().maybe_start_subtitle_window();
        }
    }

    pub(super) fn maybe_start_subtitle_window(mut self: Pin<&mut Self>) {
        if !self.media_loaded()
            || !self.subtitle_translation.enabled
            || !self.subtitle_translation_view
        {
            return;
        }
        let position_ms = (self.position * 1000.0).round() as i64;
        let Ok((start, _)) = window_bounds(position_ms) else {
            return;
        };
        if self.subtitle_window_start_ms == Some(start) {
            return;
        }
        let Some(track) = self
            .subtitle_tracks
            .rows
            .iter()
            .find(|track| Some(track.id) == self.subtitle_original_id)
            .cloned()
        else {
            return;
        };
        if !track.external && self.subtitle_demuxer.is_empty() {
            return;
        }
        let (Some(service), Some(library), Some(media)) = (
            self.subtitle_service.clone(),
            self.subtitle_library.clone(),
            self.subtitle_media.clone(),
        ) else {
            self.as_mut().rust_mut().subtitle_status = "unavailable".into();
            self.as_mut().state_changed();
            return;
        };
        self.as_mut().cancel_subtitle_job();
        self.as_mut().restore_original_subtitles();
        if let Some(id) = self.subtitle_output_id {
            self.as_mut()
                .dispatch_subtitle(&["sub-remove", &id.to_string()]);
            self.as_mut().rust_mut().subtitle_output_id = None;
        }
        self.as_mut().rust_mut().subtitle_output_path = None;
        self.as_mut().rust_mut().subtitle_output_plain = false;
        let (cancel, cancelled) = watch::channel(false);
        let generation = self.subtitle_task_generation;
        let task = WindowTask {
            service,
            generation,
            media,
            library,
            track,
            demuxer: self.subtitle_demuxer.clone(),
            origin_ms: self
                .session
                .as_ref()
                .and_then(|session| session.number("demuxer-start-time"))
                .filter(|value| value.is_finite())
                .map_or(0, |value| (value * 1000.0).round() as i64),
            position_ms,
            preferences: self.subtitle_translation.clone(),
            cancelled,
        };
        let mut state = self.as_mut().rust_mut();
        state.subtitle_task_cancel = Some(cancel);
        state.subtitle_window_start_ms = Some(start);
        state.subtitle_status = "working".into();
        ffi::start_subtitle_task(Box::new(SubtitleTask {
            thread: Box::new(self.qt_thread()),
            operation: Operation::Window(task),
        }));
        self.as_mut().state_changed();
    }

    pub(super) fn present_subtitle_output(
        mut self: Pin<&mut Self>,
        output: &Path,
        format: SubtitleFormat,
    ) {
        let path = output.to_string_lossy();
        let mut state = self.as_mut().rust_mut();
        state.subtitle_output_path = Some(output.to_path_buf());
        state.subtitle_output_plain = matches!(format, SubtitleFormat::Srt | SubtitleFormat::Vtt);
        state.subtitle_status = "working".into();
        self.as_mut()
            .dispatch_subtitle(&["sub-add", &path, "select"]);
        self.as_mut().state_changed();
    }

    pub(super) fn restore_original_subtitles(mut self: Pin<&mut Self>) {
        if self.media_loaded() {
            if let Some(id) = self.subtitle_original_id {
                self.as_mut()
                    .dispatch_subtitle(&["set", "sid", &id.to_string()]);
            }
        }
    }
}
