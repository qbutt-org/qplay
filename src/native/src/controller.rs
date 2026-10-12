use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cxx::UniquePtr;
use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use crate::preferences::Preferences;
use crate::session::{RenderLease, RetiredSession, Session};
use crate::track_model;
use crate::yt_dlp;

mod actions;
mod events;
mod gpu;
mod startup;

pub use startup::{FrameWakeup, StartTask, Wakeup, frame_wakeup, run_start_task, wakeup};

#[cxx_qt::bridge(namespace = "Qplay")]
pub mod ffi {
    #[qml_element]
    qnamespace!("Qplay::Playback");

    #[qenum]
    #[namespace = "Qplay::Playback"]
    enum State {
        Starting,
        Stopped,
        Loading,
        Playing,
        Paused,
        Failed,
    }

    #[namespace = ""]
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "C++" {
        include!("qplay_native/src/track_model.cxxqt.h");
        type TrackModel = crate::track_model::ffi::TrackModel;
        include!("qplay_native/native.h");
        #[rust_name = "start_session"]
        fn startSession(task: Box<StartTask>);
        #[rust_name = "retire_session"]
        fn retireSession(session: Box<RetiredSession>);
    }

    extern "Rust" {
        fn version_json() -> String;
        fn runtime_info_json(library: &str, data: &str, config: &str) -> Result<String>;
        type StartTask;
        type RetiredSession;
        fn run_start_task(task: Box<StartTask>);
        type RenderLease;
        fn acquire_render_lease(player: &PlayerController) -> Result<Box<RenderLease>>;
        fn render_generation(self: &RenderLease) -> u64;
        fn report_render_error(self: &RenderLease, error: &str);
        unsafe fn initialize_render(self: &mut RenderLease, resolver: usize) -> Result<()>;
        unsafe fn draw(self: &RenderLease, framebuffer: i32, width: i32, height: i32)
        -> Result<()>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(State, state, READ = state, NOTIFY = state_changed)]
        #[qproperty(bool, engine_ready, cxx_name = "engineReady", READ = engine_ready, NOTIFY = state_changed)]
        #[qproperty(bool, media_loaded, cxx_name = "mediaLoaded", READ = media_loaded, NOTIFY = state_changed)]
        #[qproperty(bool, online_waiting, cxx_name = "onlineWaiting", READ = online_waiting, NOTIFY = state_changed)]
        #[qproperty(i32, online_progress, cxx_name = "onlineProgress", READ = online_progress, NOTIFY = state_changed)]
        #[qproperty(bool, paused, READ = paused, NOTIFY = state_changed)]
        #[qproperty(bool, muted, READ = muted, NOTIFY = state_changed)]
        #[qproperty(f64, position, READ = position, NOTIFY = state_changed)]
        #[qproperty(f64, duration, READ = duration, NOTIFY = state_changed)]
        #[qproperty(f64, volume, READ = volume, NOTIFY = state_changed)]
        #[qproperty(f64, speed, READ = speed, NOTIFY = state_changed)]
        #[qproperty(f64, audio_delay, cxx_name = "audioDelay", READ = audio_delay, NOTIFY = state_changed)]
        #[qproperty(f64, applied_audio_delay, cxx_name = "appliedAudioDelay", READ = applied_audio_delay, NOTIFY = state_changed)]
        #[qproperty(QString, title, READ = title, NOTIFY = state_changed)]
        #[qproperty(QString, audio_selection, cxx_name = "audioSelection", READ = audio_selection, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_selection, cxx_name = "subtitleSelection", READ = subtitle_selection, NOTIFY = state_changed)]
        #[qproperty(QString, language, READ = language, NOTIFY = state_changed)]
        #[qproperty(QString, appearance, READ = appearance, NOTIFY = state_changed)]
        #[qproperty(QString, settings_error, cxx_name = "settingsError", READ = settings_error, NOTIFY = state_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ = error_text, NOTIFY = state_changed)]
        #[qproperty(QString, audio_output, cxx_name = "audioOutput", READ = audio_output, NOTIFY = state_changed)]
        #[qproperty(QString, hardware_decoder, cxx_name = "hardwareDecoder", READ = hardware_decoder, NOTIFY = state_changed)]
        #[qproperty(QString, shader_choice, cxx_name = "shaderChoice", READ = shader_choice, NOTIFY = state_changed)]
        #[qproperty(QString, content_choice, cxx_name = "contentChoice", READ = content_choice, NOTIFY = state_changed)]
        #[qproperty(QString, framegen_choice, cxx_name = "framegenChoice", READ = framegen_choice, NOTIFY = state_changed)]
        #[qproperty(QString, effective_shader, cxx_name = "effectiveShader", READ = effective_shader, NOTIFY = state_changed)]
        #[qproperty(QString, framegen_status, cxx_name = "framegenStatus", READ = framegen_status, NOTIFY = state_changed)]
        #[qproperty(QString, gpu_error, cxx_name = "gpuError", READ = gpu_error, NOTIFY = state_changed)]
        #[qproperty(*mut TrackModel, audio_tracks, cxx_name = "audioTracks", READ = audio_tracks_model, CONSTANT)]
        #[qproperty(*mut TrackModel, subtitle_tracks, cxx_name = "subtitleTracks", READ = subtitle_tracks_model, CONSTANT)]
        type PlayerController = super::PlayerControllerRust;

        fn state(&self) -> State;
        #[cxx_name = "engineReady"]
        fn engine_ready(&self) -> bool;
        #[cxx_name = "mediaLoaded"]
        fn media_loaded(&self) -> bool;
        #[cxx_name = "onlineWaiting"]
        fn online_waiting(&self) -> bool;
        #[cxx_name = "onlineProgress"]
        fn online_progress(&self) -> i32;
        fn paused(&self) -> bool;
        fn muted(&self) -> bool;
        fn position(&self) -> f64;
        fn duration(&self) -> f64;
        fn volume(&self) -> f64;
        fn speed(&self) -> f64;
        #[cxx_name = "audioDelay"]
        fn audio_delay(&self) -> f64;
        #[cxx_name = "appliedAudioDelay"]
        fn applied_audio_delay(&self) -> f64;
        fn title(&self) -> QString;
        #[cxx_name = "audioSelection"]
        fn audio_selection(&self) -> QString;
        #[cxx_name = "subtitleSelection"]
        fn subtitle_selection(&self) -> QString;
        fn language(&self) -> QString;
        fn appearance(&self) -> QString;
        #[cxx_name = "settingsError"]
        fn settings_error(&self) -> QString;
        #[cxx_name = "errorText"]
        fn error_text(&self) -> QString;
        #[cxx_name = "audioOutput"]
        fn audio_output(&self) -> QString;
        #[cxx_name = "hardwareDecoder"]
        fn hardware_decoder(&self) -> QString;
        #[cxx_name = "shaderChoice"]
        fn shader_choice(&self) -> QString;
        #[cxx_name = "contentChoice"]
        fn content_choice(&self) -> QString;
        #[cxx_name = "framegenChoice"]
        fn framegen_choice(&self) -> QString;
        #[cxx_name = "effectiveShader"]
        fn effective_shader(&self) -> QString;
        #[cxx_name = "framegenStatus"]
        fn framegen_status(&self) -> QString;
        #[cxx_name = "gpuError"]
        fn gpu_error(&self) -> QString;
        #[cxx_name = "audioTracks"]
        fn audio_tracks_model(&self) -> *mut TrackModel;
        #[cxx_name = "subtitleTracks"]
        fn subtitle_tracks_model(&self) -> *mut TrackModel;

        fn configure(self: Pin<&mut Self>, path: &QString, system_language: &QString);
        fn initialize(self: Pin<&mut Self>, library: &QString, profile: &QString);
        #[qinvokable]
        #[rust_name = "change_language"]
        fn changeLanguage(self: Pin<&mut Self>, language: &QString);
        #[qinvokable]
        #[rust_name = "change_appearance"]
        fn changeAppearance(self: Pin<&mut Self>, appearance: &QString);
        #[qinvokable]
        #[rust_name = "change_audio_delay"]
        fn changeAudioDelay(self: Pin<&mut Self>, delay: f64);
        #[qinvokable]
        #[rust_name = "change_shader"]
        fn changeShader(self: Pin<&mut Self>, choice: &QString);
        #[qinvokable]
        #[rust_name = "change_content"]
        fn changeContent(self: Pin<&mut Self>, choice: &QString);
        #[qinvokable]
        #[rust_name = "change_framegen"]
        fn changeFramegen(self: Pin<&mut Self>, choice: &QString);
        #[qinvokable]
        #[rust_name = "open_media"]
        fn openMedia(self: Pin<&mut Self>, media: &QString);
        #[qinvokable]
        #[rust_name = "toggle_pause"]
        fn togglePause(self: Pin<&mut Self>);
        #[qinvokable]
        fn seek(self: Pin<&mut Self>, position: f64);
        #[qinvokable]
        #[rust_name = "change_volume"]
        fn changeVolume(self: Pin<&mut Self>, volume: f64);
        #[qinvokable]
        #[rust_name = "change_mute"]
        fn changeMute(self: Pin<&mut Self>, muted: bool);
        #[qinvokable]
        #[rust_name = "change_speed"]
        fn changeSpeed(self: Pin<&mut Self>, speed: f64);
        #[qinvokable]
        #[rust_name = "select_track"]
        fn selectTrack(self: Pin<&mut Self>, kind: &QString, id: i64);
        #[qinvokable]
        #[rust_name = "select_automatic_track"]
        fn selectAutomaticTrack(self: Pin<&mut Self>, kind: &QString);
        #[qinvokable]
        #[rust_name = "disable_subtitles"]
        fn disableSubtitles(self: Pin<&mut Self>);
        #[qinvokable]
        #[rust_name = "add_subtitle"]
        fn addSubtitle(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        fn stop(self: Pin<&mut Self>);

        #[rust_name = "surface_ready"]
        fn surfaceReady(self: Pin<&mut Self>, generation: u64);
        #[rust_name = "surface_failed"]
        fn surfaceFailed(self: Pin<&mut Self>, generation: u64, error: &QString);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Self>);
        #[qsignal]
        #[cxx_name = "frameReady"]
        fn frame_ready(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for PlayerController {}
}

fn version_json() -> String {
    crate::runtime_info::version_json()
}

fn runtime_info_json(library: &str, data: &str, config: &str) -> Result<String, String> {
    crate::runtime_info::runtime_info_json(library, data, config)
}

pub struct PlayerControllerRust {
    session: Option<Arc<Session>>,
    startup_cancel: Option<Arc<AtomicBool>>,
    online_command: Option<u64>,
    online_ready: bool,
    online_progress: i32,
    online_error: Option<String>,
    generation: u64,
    surface_ready: bool,
    media: Option<String>,
    observed_path: String,
    pending_commands: HashSet<u64>,
    gpu_commands: HashMap<u64, gpu::GpuCommand>,
    gpu: gpu::GpuState,
    audio_tracks: UniquePtr<track_model::ffi::TrackModel>,
    subtitle_tracks: UniquePtr<track_model::ffi::TrackModel>,
    state: ffi::State,
    engine_ready: bool,
    file_loaded: bool,
    paused: bool,
    muted: bool,
    position: f64,
    duration: f64,
    volume: f64,
    speed: f64,
    audio_delay: f64,
    title: QString,
    audio_selection: QString,
    subtitle_selection: QString,
    language: QString,
    appearance: QString,
    settings_path: Option<PathBuf>,
    settings_error: QString,
    error_text: QString,
    audio_output: QString,
    hardware_decoder: QString,
}

impl Default for PlayerControllerRust {
    fn default() -> Self {
        Self {
            session: None,
            startup_cancel: None,
            online_command: None,
            online_ready: false,
            online_progress: -1,
            online_error: None,
            generation: 0,
            surface_ready: false,
            media: None,
            observed_path: String::new(),
            pending_commands: HashSet::new(),
            gpu_commands: HashMap::new(),
            gpu: gpu::GpuState::default(),
            audio_tracks: track_model::ffi::create_track_model(),
            subtitle_tracks: track_model::ffi::create_track_model(),
            state: ffi::State::Stopped,
            engine_ready: false,
            file_loaded: false,
            paused: false,
            muted: false,
            position: 0.0,
            duration: 0.0,
            volume: 100.0,
            speed: 1.0,
            audio_delay: 0.0,
            title: QString::default(),
            audio_selection: QString::from("auto"),
            subtitle_selection: QString::from("auto"),
            language: QString::from("en"),
            appearance: QString::from("system"),
            settings_path: None,
            settings_error: QString::default(),
            error_text: QString::default(),
            audio_output: QString::default(),
            hardware_decoder: QString::default(),
        }
    }
}

impl Drop for PlayerControllerRust {
    fn drop(&mut self) {
        if let Some(cancel) = &self.startup_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.gpu.cancel();
    }
}

impl ffi::PlayerController {
    fn state(&self) -> ffi::State {
        self.state
    }

    fn engine_ready(&self) -> bool {
        self.engine_ready
    }

    fn media_loaded(&self) -> bool {
        self.file_loaded && self.media.as_ref() == Some(&self.observed_path)
    }

    fn online_waiting(&self) -> bool {
        self.media
            .as_ref()
            .is_some_and(|media| yt_dlp::is_youtube_url(media))
            && !self.online_ready
    }

    fn online_progress(&self) -> i32 {
        self.online_progress
    }

    fn paused(&self) -> bool {
        self.paused
    }

    fn muted(&self) -> bool {
        self.muted
    }

    fn position(&self) -> f64 {
        self.position
    }

    fn duration(&self) -> f64 {
        self.duration
    }

    fn volume(&self) -> f64 {
        self.volume
    }

    fn speed(&self) -> f64 {
        self.speed
    }

    fn audio_delay(&self) -> f64 {
        self.audio_delay
    }

    fn applied_audio_delay(&self) -> f64 {
        self.session
            .as_ref()
            .and_then(|session| session.audio_delay().ok())
            .unwrap_or(0.0)
    }

    fn title(&self) -> QString {
        self.title.clone()
    }

    fn audio_selection(&self) -> QString {
        self.audio_selection.clone()
    }

    fn subtitle_selection(&self) -> QString {
        self.subtitle_selection.clone()
    }

    fn language(&self) -> QString {
        self.language.clone()
    }

    fn appearance(&self) -> QString {
        self.appearance.clone()
    }

    fn settings_error(&self) -> QString {
        self.settings_error.clone()
    }

    fn error_text(&self) -> QString {
        self.error_text.clone()
    }

    fn audio_output(&self) -> QString {
        self.audio_output.clone()
    }

    fn hardware_decoder(&self) -> QString {
        self.hardware_decoder.clone()
    }

    fn shader_choice(&self) -> QString {
        QString::from(self.gpu.shader_choice.id())
    }

    fn content_choice(&self) -> QString {
        QString::from(self.gpu.content_choice.id())
    }

    fn framegen_choice(&self) -> QString {
        QString::from(self.gpu.framegen_choice.id())
    }

    fn effective_shader(&self) -> QString {
        QString::from(self.gpu.applied_shader.id())
    }

    fn framegen_status(&self) -> QString {
        QString::from(self.gpu.framegen_status.as_str())
    }

    fn gpu_error(&self) -> QString {
        QString::from(self.gpu.error.as_str())
    }

    fn audio_tracks_model(&self) -> *mut track_model::ffi::TrackModel {
        self.audio_tracks.as_ptr().cast_mut()
    }

    fn subtitle_tracks_model(&self) -> *mut track_model::ffi::TrackModel {
        self.subtitle_tracks.as_ptr().cast_mut()
    }

    fn fail(mut self: Pin<&mut Self>, error: String) {
        let mut state = self.as_mut().rust_mut();
        state.state = ffi::State::Failed;
        state.error_text = QString::from(error);
        self.as_mut().state_changed();
    }

    fn configure(mut self: Pin<&mut Self>, path: &QString, system_language: &QString) {
        let path = PathBuf::from(path.to_string());
        let settings = Preferences::load(&path, &system_language.to_string());
        let mut state = self.as_mut().rust_mut();
        state.settings_path = Some(path);
        match settings {
            Ok(settings) => {
                state.language = QString::from(settings.language);
                state.appearance = QString::from(settings.appearance);
                state.audio_delay = settings.audio_delay;
                state.gpu.shader_choice =
                    crate::gpu::shaders::ShaderChoice::from_id(&settings.shader).unwrap();
                state.gpu.content_choice =
                    crate::gpu::content::ContentChoice::from_id(&settings.content).unwrap();
                state.gpu.framegen_choice =
                    crate::gpu::framegen::FramegenChoice::from_id(&settings.framegen).unwrap();
                state.settings_error = QString::default();
            }
            Err(error) => state.settings_error = QString::from(error),
        }
        self.as_mut().state_changed();
    }

    fn change_language(mut self: Pin<&mut Self>, language: &QString) {
        if !matches!(language.to_string().as_str(), "ru" | "en") {
            return;
        }
        self.as_mut().rust_mut().language = language.clone();
        self.as_mut().save_preferences();
    }

    fn change_appearance(mut self: Pin<&mut Self>, appearance: &QString) {
        if !matches!(appearance.to_string().as_str(), "system" | "light" | "dark") {
            return;
        }
        self.as_mut().rust_mut().appearance = appearance.clone();
        self.as_mut().save_preferences();
    }

    fn change_audio_delay(mut self: Pin<&mut Self>, delay: f64) {
        if !delay.is_finite() {
            return;
        }
        self.as_mut().rust_mut().audio_delay = delay;
        self.as_mut().save_preferences();
        if self.engine_ready {
            self.as_mut()
                .dispatch(&["set", "audio-delay", &delay.to_string()]);
        }
    }

    fn save_preferences(mut self: Pin<&mut Self>) {
        if let Some(path) = self.settings_path.clone() {
            let settings = Preferences {
                language: self.language.to_string(),
                appearance: self.appearance.to_string(),
                audio_delay: self.audio_delay,
                shader: self.gpu.shader_choice.id().into(),
                content: self.gpu.content_choice.id().into(),
                framegen: self.gpu.framegen_choice.id().into(),
            };
            self.as_mut().rust_mut().settings_error = match settings.save(&path) {
                Ok(()) => QString::default(),
                Err(error) => QString::from(error),
            };
        }
        self.as_mut().state_changed();
    }
}

fn acquire_render_lease(player: &ffi::PlayerController) -> Result<Box<RenderLease>, String> {
    let session = player
        .session
        .clone()
        .ok_or("The player is not initialized.")?;

    Ok(Box::new(RenderLease::new(session)))
}
