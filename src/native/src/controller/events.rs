use std::pin::Pin;

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
                    self.as_mut().rust_mut().file_loaded = false;
                }
                Event::FileLoaded => {
                    self.as_mut().rust_mut().file_loaded = true;
                    self.as_mut().begin_gpu_probe();
                }
                Event::EndFile(error) => {
                    self.as_mut().rust_mut().file_loaded = false;
                    if error < 0 && self.media.as_ref() == Some(&self.observed_path) {
                        self.as_mut().fail(session.error(error));
                    }
                }
                Event::Command(id, error) => {
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
                    let mut state = self.as_mut().rust_mut();
                    state.engine_ready = false;
                    state.file_loaded = false;
                    state.session = None;
                    state.state = ffi::State::Stopped;
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
            ("pause", PropertyValue::Flag(paused)) => {
                self.as_mut().rust_mut().paused = paused;
            }
            ("path", PropertyValue::Text(path)) => self.as_mut().rust_mut().observed_path = path,
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
                match serde_json::from_str::<Vec<Track>>(&json) {
                    Ok(tracks) => {
                        let (audio, subtitles): (Vec<Track>, Vec<Track>) = tracks
                            .into_iter()
                            .filter(|track| track.kind == "audio" || track.kind == "sub")
                            .partition(|track| track.kind == "audio");
                        let mut state = self.as_mut().rust_mut();
                        state.audio_tracks.pin_mut().update(audio);
                        state.subtitle_tracks.pin_mut().update(subtitles);
                    }
                    Err(error) => self.as_mut().fail(format!("Read media tracks: {error}")),
                }
            }
            _ => {}
        }
    }
}
