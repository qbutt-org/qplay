use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cxx::UniquePtr;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QString, QStringList};

use crate::app_update;
use crate::launch::LaunchOptions;
use crate::preferences::DEFAULT_SUBTITLE_POSITION;
use crate::preferences::Preferences;
use crate::preferences::SubtitleTranslationPreferences;
use crate::session::{RenderLease, RetiredSession, Session};
use crate::track_model;
use crate::yt_dlp;

mod actions;
mod events;
mod gpu;
mod startup;
mod subtitles;
mod update;

pub use startup::{FrameWakeup, StartTask, Wakeup, frame_wakeup, run_start_task, wakeup};
pub use subtitles::{SubtitleTask, run_subtitle_task};
pub use update::{UpdateTask, run_update_task};

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

    struct FramegenObservation {
        source_fps: f64,
        target_fps: f64,
        measured_fps: f64,
        output_width: u32,
        output_height: u32,
        filter_name: String,
    }

    #[namespace = ""]
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "C++" {
        include!("qplay_native/src/track_model.cxxqt.h");
        type TrackModel = crate::track_model::ffi::TrackModel;
        include!("qplay_native/native.h");
        #[rust_name = "start_session"]
        fn startSession(task: Box<StartTask>);
        #[rust_name = "retire_session"]
        fn retireSession(session: Box<RetiredSession>);
        #[rust_name = "start_update_task"]
        fn startUpdateTask(task: Box<UpdateTask>);
        #[rust_name = "start_subtitle_task"]
        fn startSubtitleTask(task: Box<SubtitleTask>);
    }

    extern "Rust" {
        fn version_json() -> String;
        fn runtime_info_json(library: &str, data: &str, config: &str) -> Result<String>;
        type StartTask;
        type RetiredSession;
        fn run_start_task(task: Box<StartTask>);
        type UpdateTask;
        fn run_update_task(task: Box<UpdateTask>);
        type SubtitleTask;
        fn run_subtitle_task(task: Box<SubtitleTask>);
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
        #[qproperty(f64, subtitle_position, cxx_name = "subtitlePosition", READ = subtitle_position, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_text, cxx_name = "subtitleText", READ = subtitle_text, NOTIFY = state_changed)]
        #[qproperty(bool, subtitle_movable, cxx_name = "subtitleMovable", READ = subtitle_movable, NOTIFY = state_changed)]
        #[qproperty(f64, applied_audio_delay, cxx_name = "appliedAudioDelay", READ = applied_audio_delay, NOTIFY = state_changed)]
        #[qproperty(QString, title, READ = title, NOTIFY = state_changed)]
        #[qproperty(QString, audio_selection, cxx_name = "audioSelection", READ = audio_selection, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_selection, cxx_name = "subtitleSelection", READ = subtitle_selection, NOTIFY = state_changed)]
        #[qproperty(bool, subtitle_translation_enabled, cxx_name = "subtitleTranslationEnabled", READ = subtitle_translation_enabled, NOTIFY = state_changed)]
        #[qproperty(bool, subtitle_translation_active, cxx_name = "subtitleTranslationActive", READ = subtitle_translation_active, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_target, cxx_name = "subtitleTarget", READ = subtitle_target, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_model, cxx_name = "subtitleModel", READ = subtitle_model, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_effort, cxx_name = "subtitleEffort", READ = subtitle_effort, NOTIFY = state_changed)]
        #[qproperty(QStringList, subtitle_models, cxx_name = "subtitleModels", READ = subtitle_models, NOTIFY = state_changed)]
        #[qproperty(QStringList, subtitle_efforts, cxx_name = "subtitleEfforts", READ = subtitle_efforts, NOTIFY = state_changed)]
        #[qproperty(QString, subtitle_status, cxx_name = "subtitleStatus", READ = subtitle_status, NOTIFY = state_changed)]
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
        #[qproperty(QString, update_state, cxx_name = "updateState", READ = update_state, NOTIFY = state_changed)]
        #[qproperty(QString, update_version, cxx_name = "updateVersion", READ = update_version, NOTIFY = state_changed)]
        #[qproperty(QString, update_url, cxx_name = "updateUrl", READ = update_url, NOTIFY = state_changed)]
        #[qproperty(QString, update_error, cxx_name = "updateError", READ = update_error, NOTIFY = state_changed)]
        #[qproperty(i32, update_progress, cxx_name = "updateProgress", READ = update_progress, NOTIFY = state_changed)]
        #[qproperty(bool, portable, READ = portable, NOTIFY = state_changed)]
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
        #[cxx_name = "subtitlePosition"]
        fn subtitle_position(&self) -> f64;
        #[cxx_name = "subtitleText"]
        fn subtitle_text(&self) -> QString;
        #[cxx_name = "subtitleMovable"]
        fn subtitle_movable(&self) -> bool;
        #[cxx_name = "appliedAudioDelay"]
        fn applied_audio_delay(&self) -> f64;
        fn title(&self) -> QString;
        #[cxx_name = "audioSelection"]
        fn audio_selection(&self) -> QString;
        #[cxx_name = "subtitleSelection"]
        fn subtitle_selection(&self) -> QString;
        #[cxx_name = "subtitleTranslationEnabled"]
        fn subtitle_translation_enabled(&self) -> bool;
        #[cxx_name = "subtitleTranslationActive"]
        fn subtitle_translation_active(&self) -> bool;
        #[cxx_name = "subtitleTarget"]
        fn subtitle_target(&self) -> QString;
        #[cxx_name = "subtitleModel"]
        fn subtitle_model(&self) -> QString;
        #[cxx_name = "subtitleEffort"]
        fn subtitle_effort(&self) -> QString;
        #[cxx_name = "subtitleModels"]
        fn subtitle_models(&self) -> QStringList;
        #[cxx_name = "subtitleEfforts"]
        fn subtitle_efforts(&self) -> QStringList;
        #[cxx_name = "subtitleStatus"]
        fn subtitle_status(&self) -> QString;
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
        #[cxx_name = "updateState"]
        fn update_state(&self) -> QString;
        #[cxx_name = "updateVersion"]
        fn update_version(&self) -> QString;
        #[cxx_name = "updateUrl"]
        fn update_url(&self) -> QString;
        #[cxx_name = "updateError"]
        fn update_error(&self) -> QString;
        #[cxx_name = "updateProgress"]
        fn update_progress(&self) -> i32;
        fn portable(&self) -> bool;
        #[cxx_name = "audioTracks"]
        fn audio_tracks_model(&self) -> *mut TrackModel;
        #[cxx_name = "subtitleTracks"]
        fn subtitle_tracks_model(&self) -> *mut TrackModel;

        fn configure(self: Pin<&mut Self>, path: &QString, system_language: &QString);
        #[cxx_name = "configureLaunch"]
        fn configure_launch(self: Pin<&mut Self>, arguments: &QStringList) -> QString;
        #[cxx_name = "configureInstallation"]
        fn configure_installation(
            self: Pin<&mut Self>,
            data: &QString,
            install: &QString,
            portable: bool,
        );
        fn initialize(self: Pin<&mut Self>, library: &QString, profile: &QString);
        #[cxx_name = "setDisplayRate"]
        fn set_display_rate(self: Pin<&mut Self>, fps: f64);
        #[cxx_name = "framegenObservation"]
        fn framegen_observation(&self) -> FramegenObservation;
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
        #[rust_name = "check_updates"]
        fn checkUpdates(self: Pin<&mut Self>);
        #[qinvokable]
        #[rust_name = "download_update"]
        fn downloadUpdate(self: Pin<&mut Self>);
        #[qinvokable]
        #[rust_name = "install_update"]
        fn installUpdate(self: Pin<&mut Self>) -> bool;
        #[qinvokable]
        #[rust_name = "preview_subtitle_position"]
        fn previewSubtitlePosition(self: Pin<&mut Self>, position: f64);
        #[qinvokable]
        #[rust_name = "commit_subtitle_position"]
        fn commitSubtitlePosition(self: Pin<&mut Self>);
        #[qinvokable]
        #[rust_name = "cancel_subtitle_position"]
        fn cancelSubtitlePosition(self: Pin<&mut Self>, position: f64);
        #[qinvokable]
        #[rust_name = "reset_subtitle_position"]
        fn resetSubtitlePosition(self: Pin<&mut Self>);
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
        #[rust_name = "change_subtitle_translation"]
        fn changeSubtitleTranslation(self: Pin<&mut Self>, enabled: bool);
        #[qinvokable]
        #[rust_name = "change_subtitle_target"]
        fn changeSubtitleTarget(self: Pin<&mut Self>, target: &QString);
        #[qinvokable]
        #[rust_name = "change_subtitle_model"]
        fn changeSubtitleModel(self: Pin<&mut Self>, model: &QString);
        #[qinvokable]
        #[rust_name = "change_subtitle_effort"]
        fn changeSubtitleEffort(self: Pin<&mut Self>, effort: &QString);
        #[qinvokable]
        #[rust_name = "use_translated_subtitles"]
        fn useTranslatedSubtitles(self: Pin<&mut Self>);
        #[qinvokable]
        #[rust_name = "load_subtitle_models"]
        fn loadSubtitleModels(self: Pin<&mut Self>);
        #[qinvokable]
        #[rust_name = "add_subtitle"]
        fn addSubtitle(self: Pin<&mut Self>, path: &QString);
        #[qinvokable]
        fn stop(self: Pin<&mut Self>);

        #[rust_name = "surface_failed"]
        fn surfaceFailed(self: Pin<&mut Self>, generation: u64, error: &QString);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Self>);
        #[qsignal]
        #[cxx_name = "frameReady"]
        fn frame_ready(self: Pin<&mut Self>);
        #[qsignal]
        #[cxx_name = "shutdownRequested"]
        fn shutdown_requested(self: Pin<&mut Self>);
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
    launch: LaunchOptions,
    startup_cancel: Option<Arc<AtomicBool>>,
    online_command: Option<u64>,
    online_ready: bool,
    online_progress: i32,
    online_error: Option<String>,
    generation: u64,
    media: Option<String>,
    observed_path: String,
    pending_commands: HashSet<u64>,
    subtitle_commands: HashMap<u64, u64>,
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
    subtitle_position: f64,
    subtitle_text: String,
    subtitle_visible: bool,
    title: QString,
    audio_selection: QString,
    subtitle_selection: QString,
    language: QString,
    appearance: QString,
    settings_path: Option<PathBuf>,
    subtitle_translation: SubtitleTranslationPreferences,
    subtitle_translation_view: bool,
    subtitle_status: QString,
    subtitle_catalog: Vec<qbutt_codex::CodexModel>,
    subtitle_catalog_requested: bool,
    subtitle_catalog_cancel: Option<tokio::sync::watch::Sender<bool>>,
    subtitle_service: Option<Arc<crate::subtitles::SubtitleService>>,
    subtitle_media: Option<Arc<crate::subtitles::PlaybackMedia>>,
    subtitle_task_cancel: Option<tokio::sync::watch::Sender<bool>>,
    subtitle_task_generation: u64,
    subtitle_original_id: Option<i64>,
    subtitle_output_id: Option<i64>,
    subtitle_output_path: Option<PathBuf>,
    subtitle_output_plain: bool,
    subtitle_library: Option<PathBuf>,
    subtitle_demuxer: String,
    subtitle_window_start_ms: Option<i64>,
    settings_error: QString,
    error_text: QString,
    audio_output: QString,
    hardware_decoder: QString,
    update_state: QString,
    update_version: QString,
    update_error: QString,
    update_progress: i32,
    update_release: Option<app_update::Release>,
    update_installer: Option<PathBuf>,
    update_data: Option<PathBuf>,
    install_directory: Option<PathBuf>,
    portable: bool,
    update_cancel: Option<Arc<AtomicBool>>,
}

impl Default for PlayerControllerRust {
    fn default() -> Self {
        Self {
            session: None,
            launch: LaunchOptions::default(),
            startup_cancel: None,
            online_command: None,
            online_ready: false,
            online_progress: -1,
            online_error: None,
            generation: 0,
            media: None,
            observed_path: String::new(),
            pending_commands: HashSet::new(),
            subtitle_commands: HashMap::new(),
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
            subtitle_position: DEFAULT_SUBTITLE_POSITION,
            subtitle_text: String::new(),
            subtitle_visible: true,
            title: QString::default(),
            audio_selection: QString::from("auto"),
            subtitle_selection: QString::from("auto"),
            language: QString::from("en"),
            appearance: QString::from("system"),
            settings_path: None,
            subtitle_translation: SubtitleTranslationPreferences::default(),
            subtitle_translation_view: false,
            subtitle_status: QString::from("idle"),
            subtitle_catalog: Vec::new(),
            subtitle_catalog_requested: false,
            subtitle_catalog_cancel: None,
            subtitle_service: None,
            subtitle_media: None,
            subtitle_task_cancel: None,
            subtitle_task_generation: 0,
            subtitle_original_id: None,
            subtitle_output_id: None,
            subtitle_output_path: None,
            subtitle_output_plain: false,
            subtitle_library: None,
            subtitle_demuxer: String::new(),
            subtitle_window_start_ms: None,
            settings_error: QString::default(),
            error_text: QString::default(),
            audio_output: QString::default(),
            hardware_decoder: QString::default(),
            update_state: QString::from("idle"),
            update_version: QString::default(),
            update_error: QString::default(),
            update_progress: 0,
            update_release: None,
            update_installer: None,
            update_data: None,
            install_directory: None,
            portable: false,
            update_cancel: None,
        }
    }
}

impl Drop for PlayerControllerRust {
    fn drop(&mut self) {
        if let Some(cancel) = &self.startup_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.gpu.cancel();
        if let Some(cancel) = &self.update_cancel {
            cancel.store(true, Ordering::Release);
        }
        if let Some(cancel) = &self.subtitle_task_cancel {
            let _ = cancel.send(true);
        }
        if let Some(cancel) = &self.subtitle_catalog_cancel {
            let _ = cancel.send(true);
        }
    }
}

impl PlayerControllerRust {
    fn supports_subtitle_position(&self) -> bool {
        if self.subtitle_translation_view && self.subtitle_output_id.is_some() {
            return self.subtitle_output_plain;
        }
        self.subtitle_tracks.rows.iter().any(|track| {
            track.selected
                && matches!(
                    track.codec.as_str(),
                    "subrip" | "webvtt" | "mov_text" | "text"
                )
        })
    }
}

impl ffi::PlayerController {
    fn configure_launch(mut self: Pin<&mut Self>, arguments: &QStringList) -> QString {
        let arguments: Vec<String> = arguments.iter().map(QString::to_string).collect();
        match LaunchOptions::parse(&arguments) {
            Ok(launch) => {
                self.as_mut().rust_mut().launch = launch;
                QString::default()
            }
            Err(error) => QString::from(error),
        }
    }

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

    fn subtitle_position(&self) -> f64 {
        self.subtitle_position
    }

    fn subtitle_text(&self) -> QString {
        if !self.subtitle_movable()
            || !self.subtitle_visible
            || self.subtitle_text.trim().is_empty()
        {
            return QString::default();
        }
        QString::from(self.subtitle_text.as_str())
    }

    fn subtitle_movable(&self) -> bool {
        self.media_loaded() && self.supports_subtitle_position()
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
                state.subtitle_position = settings.subtitle_position;
                state.subtitle_translation = settings.subtitle_translation;
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

    fn preview_subtitle_position(mut self: Pin<&mut Self>, position: f64) {
        if !self.subtitle_movable() || !position.is_finite() || !(0.0..=100.0).contains(&position) {
            return;
        }
        let position = (position * 10.0).round() / 10.0;
        if (self.subtitle_position - position).abs() < 0.1 {
            return;
        }
        self.as_mut().rust_mut().subtitle_position = position;
        self.as_mut()
            .dispatch(&["set", "sub-pos", &position.to_string()]);
        self.as_mut().state_changed();
    }

    fn commit_subtitle_position(mut self: Pin<&mut Self>) {
        if self.media_loaded() {
            self.as_mut().save_preferences();
        }
    }

    fn cancel_subtitle_position(mut self: Pin<&mut Self>, position: f64) {
        if !position.is_finite() || !(0.0..=100.0).contains(&position) {
            return;
        }
        self.as_mut().rust_mut().subtitle_position = position;
        self.as_mut().apply_selected_subtitle_position();
        self.as_mut().state_changed();
    }

    fn reset_subtitle_position(mut self: Pin<&mut Self>) {
        if !self.subtitle_movable()
            || (self.subtitle_position - DEFAULT_SUBTITLE_POSITION).abs() < 0.1
        {
            return;
        }
        self.as_mut().rust_mut().subtitle_position = DEFAULT_SUBTITLE_POSITION;
        self.as_mut().dispatch(&["set", "sub-pos", "85"]);
        self.as_mut().save_preferences();
    }

    fn save_preferences(mut self: Pin<&mut Self>) {
        if let Some(path) = self.settings_path.clone() {
            let settings = Preferences {
                language: self.language.to_string(),
                appearance: self.appearance.to_string(),
                audio_delay: self.audio_delay,
                subtitle_position: self.subtitle_position,
                subtitle_translation: self.subtitle_translation.clone(),
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
