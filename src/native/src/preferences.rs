use std::io::ErrorKind;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
pub struct SubtitleTranslationPreferences {
    pub enabled: bool,
    pub target_language: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Default for SubtitleTranslationPreferences {
    fn default() -> Self {
        Self {
            enabled: false,
            target_language: "ru".into(),
            model: None,
            effort: None,
        }
    }
}

impl SubtitleTranslationPreferences {
    fn valid(&self) -> bool {
        matches!(self.target_language.as_str(), "ru" | "en")
            && self
                .model
                .as_ref()
                .is_none_or(|value| model_token(value, 128))
            && self
                .effort
                .as_ref()
                .is_none_or(|value| model_token(value, 32))
    }
}

#[derive(Deserialize, Serialize)]
pub struct Preferences {
    pub language: String,
    pub appearance: String,
    pub audio_delay: f64,
    #[serde(default = "default_subtitle_position")]
    pub subtitle_position: f64,
    #[serde(default)]
    pub subtitle_translation: SubtitleTranslationPreferences,
    #[serde(default = "automatic")]
    pub shader: String,
    #[serde(default = "automatic")]
    pub content: String,
    #[serde(default = "automatic")]
    pub framegen: String,
}

fn automatic() -> String {
    "auto".into()
}

pub const DEFAULT_SUBTITLE_POSITION: f64 = 85.0;

fn default_subtitle_position() -> f64 {
    DEFAULT_SUBTITLE_POSITION
}

impl Preferences {
    pub fn load(path: &Path, system_language: &str) -> Result<Self, String> {
        let defaults = Self {
            language: if system_language.starts_with("ru") {
                "ru".into()
            } else {
                "en".into()
            },
            appearance: "system".into(),
            audio_delay: 0.0,
            subtitle_position: DEFAULT_SUBTITLE_POSITION,
            subtitle_translation: SubtitleTranslationPreferences::default(),
            shader: automatic(),
            content: automatic(),
            framegen: automatic(),
        };

        let contents = match std::fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(defaults),
            Err(error) => return Err(format!("Read player preferences: {error}")),
        };
        let saved: Self = serde_json::from_slice(&contents)
            .map_err(|error| format!("Read player preferences: {error}"))?;
        if !matches!(saved.language.as_str(), "ru" | "en")
            || !matches!(saved.appearance.as_str(), "system" | "light" | "dark")
            || !saved.audio_delay.is_finite()
            || !saved.subtitle_position.is_finite()
            || !(0.0..=100.0).contains(&saved.subtitle_position)
            || !saved.subtitle_translation.valid()
            || crate::gpu::shaders::ShaderChoice::from_id(&saved.shader).is_none()
            || crate::gpu::content::ContentChoice::from_id(&saved.content).is_none()
            || crate::gpu::framegen::FramegenChoice::from_id(&saved.framegen).is_none()
        {
            return Err("Player preferences contain invalid values.".into());
        }
        Ok(saved)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let directory = path
            .parent()
            .ok_or("Player preferences need a configuration directory.")?;
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("Create player configuration: {error}"))?;

        let pending = path.with_extension("tmp");
        let contents = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("Write player preferences: {error}"))?;
        std::fs::write(&pending, contents)
            .map_err(|error| format!("Write player preferences: {error}"))?;
        std::fs::rename(&pending, path).map_err(|error| format!("Save player preferences: {error}"))
    }
}

fn model_token(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
