use std::path::Path;

use super::content::ContentKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderPreset {
    Off,
    AnimeA,
    AnimeB,
    AnimeC,
    AnimeAA,
    AnimeBB,
    AnimeCA,
    Gan2,
    Gan3,
    Gan4,
    Fsr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderChoice {
    Auto,
    Manual(ShaderPreset),
}

impl ShaderChoice {
    pub fn from_id(id: &str) -> Option<Self> {
        if id == "auto" {
            Some(Self::Auto)
        } else {
            ShaderPreset::from_id(id).map(Self::Manual)
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual(preset) => preset.id(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderQuality {
    High,
    Fast,
}

#[derive(Clone, Copy, Debug)]
pub struct VideoInfo {
    pub has_video: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub display_width: u32,
    pub display_height: u32,
    pub video_bitrate: f64,
    pub file_size: f64,
    pub duration: f64,
    pub content_kind: ContentKind,
}

#[derive(Clone, Copy, Debug)]
pub struct AutoOptions {
    pub max_gan_scale: u8,
}

impl ShaderPreset {
    pub const ALL: [Self; 11] = [
        Self::Off,
        Self::AnimeA,
        Self::AnimeB,
        Self::AnimeC,
        Self::AnimeAA,
        Self::AnimeBB,
        Self::AnimeCA,
        Self::Gan2,
        Self::Gan3,
        Self::Gan4,
        Self::Fsr,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::AnimeA => "anime4k_a",
            Self::AnimeB => "anime4k_b",
            Self::AnimeC => "anime4k_c",
            Self::AnimeAA => "anime4k_aa",
            Self::AnimeBB => "anime4k_bb",
            Self::AnimeCA => "anime4k_ca",
            Self::Gan2 => "anime4k_gan_2x",
            Self::Gan3 => "anime4k_gan_3x",
            Self::Gan4 => "anime4k_gan_4x",
            Self::Fsr => "fsr",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.id() == id)
    }

    pub fn files(self, quality: ShaderQuality) -> &'static [&'static str] {
        use ShaderPreset::*;
        match (self, quality) {
            (Off, _) => &[],
            (AnimeA, ShaderQuality::High) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_VL.glsl",
                "Anime4K_Upscale_CNN_x2_VL.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            (AnimeB, ShaderQuality::High) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_VL.glsl",
                "Anime4K_Upscale_CNN_x2_VL.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            (AnimeC, ShaderQuality::High) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Upscale_Denoise_CNN_x2_VL.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            (AnimeAA, ShaderQuality::High) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_VL.glsl",
                "Anime4K_Upscale_CNN_x2_VL.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            (AnimeBB, ShaderQuality::High) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_VL.glsl",
                "Anime4K_Upscale_CNN_x2_VL.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Restore_CNN_Soft_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            (AnimeCA, ShaderQuality::High) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Upscale_Denoise_CNN_x2_VL.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
            ],
            (AnimeA, ShaderQuality::Fast) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_S.glsl",
            ],
            (AnimeB, ShaderQuality::Fast) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_S.glsl",
            ],
            (AnimeC, ShaderQuality::Fast) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Upscale_Denoise_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_S.glsl",
            ],
            (AnimeAA, ShaderQuality::Fast) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_Restore_CNN_S.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Upscale_CNN_x2_S.glsl",
            ],
            (AnimeBB, ShaderQuality::Fast) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_CNN_Soft_M.glsl",
                "Anime4K_Upscale_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Restore_CNN_Soft_S.glsl",
                "Anime4K_Upscale_CNN_x2_S.glsl",
            ],
            (AnimeCA, ShaderQuality::Fast) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Upscale_Denoise_CNN_x2_M.glsl",
                "Anime4K_AutoDownscalePre_x2.glsl",
                "Anime4K_AutoDownscalePre_x4.glsl",
                "Anime4K_Restore_CNN_S.glsl",
                "Anime4K_Upscale_CNN_x2_S.glsl",
            ],
            (Gan2, _) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_GAN_UL.glsl",
                "Anime4K_Upscale_GAN_x2_M.glsl",
            ],
            (Gan3, _) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_GAN_UL.glsl",
                "Anime4K_Upscale_GAN_x3_L.glsl",
            ],
            (Gan4, _) => &[
                "Anime4K_Clamp_Highlights.glsl",
                "Anime4K_Restore_GAN_UUL.glsl",
                "Anime4K_Upscale_GAN_x4_UL.glsl",
            ],
            (Fsr, _) => &["FSR.glsl"],
        }
    }

    pub fn mpv_value(self, quality: ShaderQuality, shader_dir: &Path) -> Result<String, String> {
        self.files(quality)
            .iter()
            .map(|name| {
                let path = shader_dir.join(name);
                if !path.is_file() {
                    return Err(format!("The shader is missing: {}", path.display()));
                }
                path.to_str().map(str::to_owned).ok_or_else(|| {
                    format!("The shader path is not valid UTF-8: {}", path.display())
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|paths| paths.join(";"))
    }
}

impl VideoInfo {
    fn scale(self) -> f64 {
        if self.display_width > 0 && self.display_height > 0 {
            let source_short = self.width.min(self.height) as f64;
            let source_long = self.width.max(self.height) as f64;
            let display_short = self.display_width.min(self.display_height) as f64;
            let display_long = self.display_width.max(self.display_height) as f64;
            return (display_short / source_short).max(display_long / source_long);
        }

        match self.width.min(self.height) {
            0..=540 => 4.0,
            541..=720 => 3.0,
            721..=1080 => 2.0,
            1081..=1440 => 1.5,
            _ => 1.0,
        }
    }

    fn degradation_score(self) -> u8 {
        let short = self.width.min(self.height);
        let mut score = if short <= 540 {
            2
        } else if short <= 720 {
            1
        } else {
            0
        };
        let bitrate = if self.video_bitrate > 0.0 {
            self.video_bitrate
        } else if self.file_size > 0.0 && self.duration > 0.0 {
            self.file_size * 8.0 / self.duration * 0.85
        } else {
            0.0
        };
        if self.fps > 0.0 && bitrate > 0.0 {
            let bpp = bitrate / (self.width as f64 * self.height as f64 * self.fps);
            score += if bpp < 0.055 {
                3
            } else if bpp < 0.085 {
                2
            } else if bpp < 0.12 {
                1
            } else {
                0
            };
        }
        score
    }
}

pub fn choose_preset(
    choice: ShaderChoice,
    video: VideoInfo,
    frame_generation_active: bool,
    options: AutoOptions,
) -> ShaderPreset {
    if let ShaderChoice::Manual(preset) = choice {
        return if frame_generation_active && preset != ShaderPreset::Fsr {
            ShaderPreset::Fsr
        } else {
            preset
        };
    }
    if !video.has_video || video.width == 0 || video.height == 0 {
        return ShaderPreset::Off;
    }
    if frame_generation_active || video.content_kind != ContentKind::Anime {
        return ShaderPreset::Fsr;
    }

    let short = video.width.min(video.height);
    let scale = video.scale();
    let degraded = video.degradation_score();
    if short <= 540 {
        if scale >= 2.8 && degraded >= 3 {
            if scale >= 3.7 && options.max_gan_scale >= 4 {
                return ShaderPreset::Gan4;
            }
            if options.max_gan_scale >= 3 {
                return ShaderPreset::Gan3;
            }
            if options.max_gan_scale >= 2 {
                return ShaderPreset::Gan2;
            }
        }
        if scale >= 2.0 {
            return if degraded >= 3 {
                ShaderPreset::AnimeAA
            } else {
                ShaderPreset::AnimeCA
            };
        }
        return if degraded >= 3 {
            ShaderPreset::AnimeA
        } else {
            ShaderPreset::AnimeC
        };
    }
    if short <= 720 {
        if scale >= 2.8 && degraded >= 4 && options.max_gan_scale >= 2 {
            return ShaderPreset::Gan2;
        }
        if scale >= 2.0 {
            return if degraded >= 3 {
                ShaderPreset::AnimeAA
            } else {
                ShaderPreset::AnimeBB
            };
        }
        return ShaderPreset::AnimeB;
    }
    if short <= 1080 && scale >= 2.0 && degraded >= 2 {
        return ShaderPreset::AnimeAA;
    }
    ShaderPreset::AnimeA
}
