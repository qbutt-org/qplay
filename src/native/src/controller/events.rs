use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use super::ffi;
use crate::session::{Event, PropertyValue};
use crate::track_model::Track;

impl ffi::PlayerController {
    pub(super) fn drain_events(mut self: Pin<&mut Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let mut count = 0;
        while count < 128 {
            let Some(event) = session.poll() else {
                break;
            };
            count += 1;
            match event {
                Event::Property(name, value) => self.as_mut().apply_property(&name, value),
                Event::StartFile => {
                    self.as_mut().cancel_subtitle_job();
                    let mut state = self.as_mut().rust_mut();
                    state.subtitle_original_id = None;
                    state.subtitle_output_id = None;
                    state.subtitle_output_path = None;
                    state.subtitle_media = None;
                    state.subtitle_demuxer.clear();
                    state.subtitle_translation_view = state.subtitle_translation.enabled;
                    state.subtitle_status = "idle".into();
                    self.as_mut().rust_mut().file_loaded = false;
                    self.as_mut().rust_mut().subtitle_text.clear();
                    self.as_mut().dispatch(&["set", "sub-pos", "100"]);
                }
                Event::FileLoaded => {
                    if self.launch.ipc {
                        if let Some(path) = session.text("path") {
                            self.as_mut().rust_mut().media = Some(path.clone());
                            self.as_mut().rust_mut().observed_path = path;
                        }
                    }
                    let mut state = self.as_mut().rust_mut();
                    state.subtitle_media = state.media.as_ref().map(|media| {
                        let mut headers = session
                            .string_array("http-header-fields")
                            .unwrap_or_default();
                        for (property, name) in
                            [("user-agent", "User-Agent"), ("referrer", "Referer")]
                        {
                            if headers.iter().any(|header| {
                                header
                                    .split_once(':')
                                    .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case(name))
                            }) {
                                continue;
                            }
                            if let Some(value) =
                                session.text(property).filter(|value| !value.is_empty())
                            {
                                headers.push(format!("{name}: {value}"));
                            }
                        }
                        Arc::new(crate::subtitles::PlaybackMedia::new(
                            media.clone(),
                            headers,
                            session.text("http-proxy"),
                        ))
                    });
                    state.file_loaded = true;
                    state.error_text = QString::default();
                    state.state = ffi::State::Loading;
                    self.as_mut().begin_gpu_probe();
                    self.as_mut().maybe_start_subtitle_window();
                }
                Event::EndFile(error) => {
                    self.as_mut().cancel_subtitle_job();
                    self.as_mut().rust_mut().file_loaded = false;
                    self.as_mut().rust_mut().subtitle_media = None;
                    self.as_mut().rust_mut().subtitle_text.clear();
                    if error < 0
                        && (self.launch.ipc || self.media.as_ref() == Some(&self.observed_path))
                    {
                        self.as_mut().fail(session.error(error));
                    } else if self.state != ffi::State::Failed {
                        self.as_mut().rust_mut().state = ffi::State::Stopped;
                    }
                }
                Event::Command(id, error) => {
                    if let Some(generation) = self.as_mut().rust_mut().subtitle_commands.remove(&id)
                    {
                        if error < 0 && generation == self.subtitle_task_generation {
                            let mut state = self.as_mut().rust_mut();
                            state.subtitle_status = "failed".into();
                            state.subtitle_translation_view = false;
                        }
                        continue;
                    }
                    if self.online_command == Some(id) {
                        self.as_mut().rust_mut().online_command = None;
                        if error < 0 {
                            let error =
                                format!("Configure online playback: {}", session.error(error));
                            self.as_mut().rust_mut().online_error = Some(error.clone());
                            if self.online_waiting() {
                                self.as_mut().fail(error);
                            }
                        } else {
                            let waiting = self.online_waiting();
                            let mut state = self.as_mut().rust_mut();
                            state.online_ready = true;
                            if waiting {
                                self.as_mut().load_pending_media();
                            }
                        }
                        continue;
                    }
                    let known = self.as_mut().rust_mut().pending_commands.remove(&id);
                    if known && error < 0 {
                        self.as_mut().fail(session.error(error));
                    }
                    if let Some(action) = self.as_mut().rust_mut().gpu_commands.remove(&id) {
                        self.as_mut().gpu_command_complete(action, error);
                    }
                }
                Event::Shutdown => {
                    self.as_mut().cancel_subtitle_job();
                    let mut state = self.as_mut().rust_mut();
                    state.engine_ready = false;
                    state.file_loaded = false;
                    state.subtitle_text.clear();
                    state.session = None;
                    state.state = ffi::State::Stopped;
                    self.as_mut().shutdown_requested();
                }
                _ => {}
            }
        }
        if self.media_loaded() && self.state != ffi::State::Failed {
            self.as_mut().rust_mut().state = if self.paused {
                ffi::State::Paused
            } else {
                ffi::State::Playing
            };
        }
        if count == 128 {
            let generation = self.generation;
            let _ = self.qt_thread().queue(move |mut player| {
                if player.generation == generation {
                    player.as_mut().drain_events();
                }
            });
        }
        if count > 0 {
            self.as_mut().state_changed();
        }
    }

    fn apply_property(mut self: Pin<&mut Self>, name: &str, value: PropertyValue) {
        match (name, value) {
            ("time-pos", PropertyValue::Number(position)) => {
                self.as_mut().rust_mut().position = position;
                self.as_mut().tick_gpu_quality();
                self.as_mut().maybe_start_subtitle_window();
            }
            ("time-pos", PropertyValue::Unavailable) => self.as_mut().rust_mut().position = 0.0,
            ("duration", PropertyValue::Number(duration)) => {
                self.as_mut().rust_mut().duration = duration
            }
            ("duration", PropertyValue::Unavailable) => self.as_mut().rust_mut().duration = 0.0,
            ("volume", PropertyValue::Number(volume)) => self.as_mut().rust_mut().volume = volume,
            ("speed", PropertyValue::Number(speed)) => self.as_mut().rust_mut().speed = speed,
            ("mute", PropertyValue::Flag(muted)) => self.as_mut().rust_mut().muted = muted,
            ("options/aid", PropertyValue::Text(selection)) => {
                self.as_mut().rust_mut().audio_selection = QString::from(selection)
            }
            ("options/sid", PropertyValue::Text(selection)) => {
                self.as_mut().rust_mut().subtitle_selection = QString::from(selection)
            }
            ("sub-text", PropertyValue::Text(text)) => {
                let text =
                    if text.chars().take(513).count() <= 512 && text.lines().take(5).count() <= 4 {
                        text
                    } else {
                        String::new()
                    };
                self.as_mut().rust_mut().subtitle_text = text;
            }
            ("sub-text", PropertyValue::Unavailable) => {
                self.as_mut().rust_mut().subtitle_text.clear();
            }
            ("sub-visibility", PropertyValue::Flag(visible)) => {
                self.as_mut().rust_mut().subtitle_visible = visible;
            }
            ("pause", PropertyValue::Flag(paused)) => {
                self.as_mut().rust_mut().paused = paused;
            }
            ("path", PropertyValue::Text(path)) => self.as_mut().rust_mut().observed_path = path,
            ("current-demuxer", PropertyValue::Text(demuxer)) => {
                self.as_mut().rust_mut().subtitle_demuxer = demuxer;
                self.as_mut().maybe_start_subtitle_window();
            }
            ("media-title", PropertyValue::Text(title)) => {
                self.as_mut().rust_mut().title = QString::from(title)
            }
            ("current-ao", PropertyValue::Text(output)) => {
                self.as_mut().rust_mut().audio_output = QString::from(output)
            }
            ("hwdec-current", PropertyValue::Text(decoder)) => {
                self.as_mut().rust_mut().hardware_decoder = QString::from(decoder)
            }
            ("video-frame-info/interlaced", PropertyValue::Flag(true))
                if !self.gpu.interlaced_seen =>
            {
                self.as_mut().cancel_gpu();
                self.as_mut().rust_mut().gpu.interlaced_seen = true;
                self.as_mut().rust_mut().gpu.framegen_status = "guarded".into();
                self.as_mut().apply_gpu_policy();
            }
            ("track-list", PropertyValue::Text(json)) => {
                let signature = super::gpu::video_signature(&json);
                match serde_json::from_str::<Vec<Track>>(&json) {
                    Ok(tracks) => {
                        let output = self.subtitle_output_path.as_ref();
                        let service = self.subtitle_service.as_ref();
                        let generated = |track: &Track| {
                            track.external_filename.as_deref().is_some_and(|name| {
                                let path = std::path::Path::new(name);
                                service.is_some_and(|service| service.owns_output_path(path))
                            })
                        };
                        let output_id = tracks
                            .iter()
                            .find(|track| {
                                track.kind == "sub"
                                    && output.is_some_and(|path| {
                                        track.external_filename.as_deref().map(std::path::Path::new)
                                            == Some(path.as_path())
                                    })
                            })
                            .map(|track| track.id);
                        let original_id = tracks
                            .iter()
                            .find(|track| {
                                track.kind == "sub" && track.selected && !generated(track)
                            })
                            .map(|track| track.id);
                        let previous_original = self.subtitle_original_id.filter(|id| {
                            tracks.iter().any(|track| {
                                track.kind == "sub" && track.id == *id && !generated(track)
                            })
                        });
                        let (audio, subtitles): (Vec<Track>, Vec<Track>) = tracks
                            .into_iter()
                            .filter(|track| {
                                (track.kind == "audio" || track.kind == "sub") && !generated(track)
                            })
                            .partition(|track| track.kind == "audio");
                        let mut state = self.as_mut().rust_mut();
                        state.subtitle_output_id = output_id;
                        state.subtitle_original_id = original_id.or(previous_original);
                        state.audio_tracks.pin_mut().update(audio);
                        state.subtitle_tracks.pin_mut().update(subtitles);
                        self.as_mut().apply_selected_subtitle_position();
                        self.as_mut().maybe_start_subtitle_window();
                    }
                    Err(error) => self.as_mut().fail(format!("Read media tracks: {error}")),
                }
                if self.file_loaded && signature.is_some() && signature != self.gpu.video_signature
                {
                    self.as_mut().begin_gpu_probe();
                }
            }
            _ => {}
        }
    }

    pub(super) fn apply_selected_subtitle_position(mut self: Pin<&mut Self>) {
        let position = if self.supports_subtitle_position() {
            self.subtitle_position.to_string()
        } else {
            "100".into()
        };
        self.as_mut().dispatch(&["set", "sub-pos", &position]);
    }
}
