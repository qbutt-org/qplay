use std::path::Path;

use crate::gpu::content::ContentChoice;

#[derive(Clone, Default)]
pub struct LaunchOptions {
    pub ipc_path: Option<String>,
    pub external_downloads: bool,
    pub ytdl_path: Option<String>,
    pub content: Option<ContentChoice>,
    options: Vec<(String, String)>,
    ytdl_options: Vec<(String, String)>,
    headers: Vec<String>,
    audio: Vec<String>,
    subtitles: Vec<String>,
}

impl LaunchOptions {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        if arguments.len() > 64 || arguments.iter().map(String::len).sum::<usize>() > 64 * 1024 {
            return Err("Too many playback options.".into());
        }

        let mut launch = Self::default();
        for argument in arguments {
            if argument.len() > 16 * 1024 || argument.contains('\0') {
                return Err("Invalid playback option.".into());
            }
            let Some((name, value)) = argument
                .strip_prefix("--")
                .and_then(|arg| arg.split_once('='))
            else {
                if argument == "--external-downloads" {
                    launch.external_downloads = true;
                    continue;
                }
                return Err(format!("Unsupported playback option: {argument}"));
            };

            match name {
                "input-ipc-server" => {
                    if !value.starts_with(r"\\.\pipe\qbutt-mpv-") || value.len() > 256 {
                        return Err("Invalid player IPC path.".into());
                    }
                    launch.ipc_path = Some(value.into());
                }
                "idle"
                | "terminal"
                | "cookies"
                | "ytdl"
                | "resume-playback"
                | "save-position-on-quit" => {
                    if !matches!(value, "yes" | "no") {
                        return Err(format!("Invalid {name} value."));
                    }
                    launch.options.push((name.into(), value.into()));
                }
                "player-operation-mode" if value == "pseudo-gui" => {}
                "http-proxy" => launch.options.push((name.into(), value.into())),
                "cookies-file" => {
                    if !Path::new(value).is_absolute() {
                        return Err("Cookie file must have an absolute path.".into());
                    }
                    launch.options.push((name.into(), value.into()));
                }
                "force-media-title" => launch.options.push((name.into(), value.into())),
                "start" => {
                    let position = value
                        .trim_start_matches('+')
                        .parse::<f64>()
                        .map_err(|_| "Invalid playback position.")?;
                    if !position.is_finite() || position < 0.0 {
                        return Err("Invalid playback position.".into());
                    }
                    launch.options.push((name.into(), value.into()));
                }
                "script-opts-append" => {
                    if let Some(path) = value.strip_prefix("ytdl_hook-ytdl_path=") {
                        if !Path::new(path).is_absolute() {
                            return Err("yt-dlp needs an absolute path.".into());
                        }
                        launch.ytdl_path = Some(path.into());
                    } else if let Some(content) = value.strip_prefix("video_profile-override=") {
                        launch.content = Some(
                            ContentChoice::from_id(content)
                                .filter(|choice| *choice != ContentChoice::Auto)
                                .ok_or("Unsupported content profile.")?,
                        );
                    } else {
                        return Err("Unsupported script option.".into());
                    }
                }
                "ytdl-raw-options-append" => {
                    let Some((key, option)) = value.split_once('=') else {
                        return Err("Unsupported extractor option.".into());
                    };
                    match key {
                        "ignore-config" | "no-plugin-dirs" if option.is_empty() => {}
                        "js-runtimes"
                            if option
                                .strip_prefix("deno:")
                                .is_some_and(|path| Path::new(path).is_absolute()) => {}
                        "cookies" if Path::new(option).is_absolute() => {}
                        _ => return Err("Unsupported extractor option.".into()),
                    }
                    launch.ytdl_options.push((key.into(), option.into()));
                }
                "http-header-fields-append" => {
                    let Some(specification) = value.strip_prefix('%') else {
                        return Err("Invalid HTTP header option.".into());
                    };
                    let Some((length, header)) = specification.split_once('%') else {
                        return Err("Invalid HTTP header option.".into());
                    };
                    if length.parse::<usize>().ok() != Some(header.len())
                        || header.bytes().any(|byte| byte < 32 || byte == 127)
                        || !header.contains(": ")
                    {
                        return Err("Invalid HTTP header option.".into());
                    }
                    launch.headers.push(header.into());
                }
                "audio-file" | "sub-file" => {
                    if value.is_empty() {
                        return Err("Empty external track.".into());
                    }
                    let tracks = if name == "audio-file" {
                        &mut launch.audio
                    } else {
                        &mut launch.subtitles
                    };
                    if tracks.len() >= 16 {
                        return Err("Too many external tracks.".into());
                    }
                    tracks.push(value.into());
                }
                _ => return Err(format!("Unsupported playback option: --{name}")),
            }
        }
        Ok(launch)
    }

    pub fn mpv_options(&self) -> &[(String, String)] {
        &self.options
    }

    pub fn mpv_array_options(&self) -> Vec<(&'static str, &[String])> {
        let mut options = Vec::new();
        for (name, values) in [
            ("http-header-fields", &self.headers),
            ("audio-files", &self.audio),
            ("sub-files", &self.subtitles),
        ] {
            if !values.is_empty() {
                options.push((name, values.as_slice()));
            }
        }
        options
    }

    pub fn mpv_map_options(
        &self,
        default_ytdl_path: &Path,
    ) -> Vec<(&'static str, Vec<(String, String)>)> {
        let ytdl_path = self
            .ytdl_path
            .as_deref()
            .map(Path::new)
            .unwrap_or(default_ytdl_path);
        let mut options = vec![(
            "script-opts",
            vec![(
                "ytdl_hook-ytdl_path".into(),
                ytdl_path.to_string_lossy().replace('\\', "/"),
            )],
        )];
        if !self.ytdl_options.is_empty() {
            options.push(("ytdl-raw-options", self.ytdl_options.clone()));
        }
        options
    }
}
