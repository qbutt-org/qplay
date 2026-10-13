use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use super::ffi;
use crate::session::media_path;
use crate::yt_dlp::is_youtube_url;

impl ffi::PlayerController {
    pub(super) fn dispatch(mut self: Pin<&mut Self>, arguments: &[&str]) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.pending_commands.len() >= 32 {
            self.as_mut().fail("The player is busy. Try again.".into());
            return;
        }
        match session.command(arguments) {
            Ok(id) => {
                self.as_mut().rust_mut().pending_commands.insert(id);
            }
            Err(error) => self.as_mut().fail(error),
        }
    }

    pub(super) fn load_pending_media(mut self: Pin<&mut Self>) {
        if self.session.is_none() {
            return;
        }
        let Some(media) = self.media.clone() else {
            return;
        };
        if is_youtube_url(&media) && !self.online_ready {
            return;
        }
        self.as_mut().rust_mut().state = ffi::State::Loading;
        self.as_mut().dispatch(&["loadfile", &media, "replace"]);
        self.as_mut().state_changed();
    }

    pub(super) fn open_media(mut self: Pin<&mut Self>, media: &QString) {
        match media_path(&media.to_string()) {
            Ok(media) => {
                self.as_mut().cancel_gpu();
                self.as_mut().cancel_subtitle_job();
                let waiting = is_youtube_url(&media) && !self.online_ready;
                let mut state = self.as_mut().rust_mut();
                state.gpu.interlaced_seen = false;
                state.gpu.video_signature = None;
                state.media = Some(media);
                state.subtitle_media = None;
                state.online_progress = -1;
                state.file_loaded = false;
                state.pending_commands.clear();
                state.observed_path.clear();
                state.position = 0.0;
                state.duration = 0.0;
                state.title = QString::default();
                state.subtitle_text.clear();
                state.error_text = QString::default();
                state.audio_tracks.pin_mut().update(Vec::new());
                state.subtitle_tracks.pin_mut().update(Vec::new());
                if waiting {
                    self.as_mut().rust_mut().state = ffi::State::Loading;
                    if let Some(error) = self.online_error.clone() {
                        self.as_mut().fail(error);
                    } else {
                        self.as_mut().state_changed();
                    }
                } else {
                    self.as_mut().load_pending_media();
                }
            }
            Err(error) => self.as_mut().fail(error),
        }
    }

    pub(super) fn toggle_pause(mut self: Pin<&mut Self>) {
        if self.media_loaded() {
            self.as_mut().dispatch(&["cycle", "pause"]);
        }
    }

    pub(super) fn seek(mut self: Pin<&mut Self>, position: f64) {
        if self.media_loaded() && position.is_finite() && position >= 0.0 {
            if self.subtitle_translation_view {
                self.as_mut().cancel_subtitle_job();
                self.as_mut().restore_original_subtitles();
                self.as_mut().rust_mut().position = position;
            }
            if let Some(quality) = self.as_mut().rust_mut().gpu.quality.as_mut() {
                quality.filter_changed();
            }
            self.as_mut()
                .dispatch(&["seek", &position.to_string(), "absolute+exact"]);
            self.as_mut().maybe_start_subtitle_window();
        }
    }

    pub(super) fn change_volume(mut self: Pin<&mut Self>, volume: f64) {
        if volume.is_finite() && (0.0..=100.0).contains(&volume) {
            self.as_mut()
                .dispatch(&["set", "volume", &volume.to_string()]);
        }
    }

    pub(super) fn change_mute(mut self: Pin<&mut Self>, muted: bool) {
        self.as_mut()
            .dispatch(&["set", "mute", if muted { "yes" } else { "no" }]);
    }

    pub(super) fn change_speed(mut self: Pin<&mut Self>, speed: f64) {
        if speed.is_finite() && (0.25..=4.0).contains(&speed) {
            if let Some(quality) = self.as_mut().rust_mut().gpu.quality.as_mut() {
                quality.filter_changed();
            }
            self.as_mut()
                .dispatch(&["set", "speed", &speed.to_string()]);
        }
    }

    pub(super) fn select_track(mut self: Pin<&mut Self>, kind: &QString, id: i64) {
        let kind = kind.to_string();
        let (tracks, property) = match kind.as_str() {
            "audio" => (&self.audio_tracks.rows, "aid"),
            "sub" => (&self.subtitle_tracks.rows, "sid"),
            _ => return,
        };
        let valid = tracks.iter().any(|track| track.id == id);
        if !valid || !self.media_loaded() {
            return;
        }
        if kind == "sub" {
            self.as_mut().cancel_subtitle_job();
            let mut state = self.as_mut().rust_mut();
            state.subtitle_original_id = Some(id);
            state.subtitle_translation_view = false;
            state.subtitle_status = "idle".into();
        }
        self.as_mut().dispatch(&["set", property, &id.to_string()]);
        self.as_mut().state_changed();
    }

    pub(super) fn select_automatic_track(mut self: Pin<&mut Self>, kind: &QString) {
        if !self.media_loaded() {
            return;
        }
        let property = match kind.to_string().as_str() {
            "audio" => "aid",
            "sub" => "sid",
            _ => return,
        };
        if property == "sid" {
            self.as_mut().cancel_subtitle_job();
            self.as_mut().rust_mut().subtitle_translation_view = false;
        }
        self.as_mut().dispatch(&["set", property, "auto"]);
        self.as_mut().state_changed();
    }

    pub(super) fn disable_subtitles(mut self: Pin<&mut Self>) {
        if self.media_loaded() {
            self.as_mut().cancel_subtitle_job();
            self.as_mut().rust_mut().subtitle_translation_view = false;
            self.as_mut().dispatch(&["set", "sid", "no"]);
            self.as_mut().state_changed();
        }
    }

    pub(super) fn add_subtitle(mut self: Pin<&mut Self>, path: &QString) {
        match media_path(&path.to_string()) {
            Ok(path) => {
                self.as_mut().cancel_subtitle_job();
                self.as_mut().rust_mut().subtitle_translation_view = false;
                self.as_mut().dispatch(&["sub-add", &path, "select"]);
                self.as_mut().state_changed();
            }
            Err(error) => self.as_mut().fail(error),
        }
    }

    pub(super) fn stop(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_gpu();
        self.as_mut().cancel_subtitle_job();
        let mut state = self.as_mut().rust_mut();
        state.media = None;
        state.subtitle_media = None;
        state.file_loaded = false;
        state.pending_commands.clear();
        state.state = ffi::State::Stopped;
        state.title = QString::default();
        state.subtitle_text.clear();
        state.audio_tracks.pin_mut().update(Vec::new());
        state.subtitle_tracks.pin_mut().update(Vec::new());
        self.as_mut().dispatch(&["stop"]);
        self.as_mut().state_changed();
    }
}
