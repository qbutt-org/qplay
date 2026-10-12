use std::io::ErrorKind;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Preferences {
    pub language: String,
    pub appearance: String,
    pub audio_delay: f64,
    #[serde(default = "default_subtitle_position")]
    pub subtitle_position: f64,
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
