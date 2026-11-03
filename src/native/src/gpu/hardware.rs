#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIFactory1};

use super::shaders::ShaderQuality;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HardwareTier {
    Lite,
    Balanced,
    High,
    Ultra,
}

#[derive(Clone, Debug)]
pub struct HardwareProfile {
    pub tier: HardwareTier,
    pub quality: ShaderQuality,
    pub max_gan_scale: u8,
}

fn model_after(name: &str, marker: &str) -> Option<u32> {
    let (_, suffix) = name.split_once(marker)?;
    let digits: String = suffix
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn profile(name: &str, vendor: u32, memory_bytes: u64, software: bool) -> HardwareProfile {
    let lower = name.to_ascii_lowercase();
    let vram_mib = memory_bytes / (1024 * 1024);
    let integrated = vram_mib == 0 && matches!(vendor, 0x1002 | 0x1022 | 0x8086);
    let tier = if software || lower.contains("basic render") || lower.contains("remote") {
        HardwareTier::Lite
    } else if integrated {
        HardwareTier::Balanced
    } else if vendor == 0x10de {
        if let Some(model) = model_after(&lower, "rtx") {
            let generation = model / 100;
            let class = model % 100;
            if class >= 90
                || generation >= 40 && class >= 80
                || generation >= 50 && class >= 70
                || vram_mib >= 16_000
            {
                HardwareTier::Ultra
            } else if generation >= 30 && class >= 60 || vram_mib >= 8_000 {
                HardwareTier::High
            } else {
                HardwareTier::Balanced
            }
        } else if let Some(model) = model_after(&lower, "gtx") {
            let generation = model / 100;
            let class = model % 100;
            if generation >= 16 && class >= 60 || generation >= 10 && class >= 70 {
                HardwareTier::High
            } else {
                HardwareTier::Balanced
            }
        } else {
            tier_from_memory(vram_mib)
        }
    } else if vendor == 0x1002 || vendor == 0x1022 {
        if let Some(model) = model_after(&lower, "rx") {
            let class = if model % 1000 >= 100 {
                model % 1000 / 10
            } else {
                model % 1000
            };
            if class >= 90 || class >= 80 && lower.contains("xtx") || vram_mib >= 16_000 {
                HardwareTier::Ultra
            } else if class >= 70 || vram_mib >= 8_000 {
                HardwareTier::High
            } else {
                HardwareTier::Balanced
            }
        } else {
            HardwareTier::Balanced
        }
    } else if vendor == 0x8086 {
        if let Some(model) = model_after(&lower, "arc a").or_else(|| model_after(&lower, "arc b")) {
            if model >= 700 {
                HardwareTier::High
            } else if model >= 500 {
                HardwareTier::High
            } else {
                HardwareTier::Balanced
            }
        } else {
            HardwareTier::Balanced
        }
    } else {
        tier_from_memory(vram_mib)
    };
    let quality = if tier >= HardwareTier::High {
        ShaderQuality::High
    } else {
        ShaderQuality::Fast
    };
    let gan_eligible = !software
        && !integrated
        && !lower.contains("gtx")
        && !lower.contains("basic render")
        && !lower.contains("remote")
        && (vendor != 0x8086
            || model_after(&lower, "arc a").is_some_and(|model| model >= 700)
            || model_after(&lower, "arc b").is_some_and(|model| model >= 700));
    let max_gan_scale = if !gan_eligible {
        0
    } else {
        match tier {
            HardwareTier::Lite | HardwareTier::Balanced => 0,
            HardwareTier::High => 2,
            HardwareTier::Ultra => 4,
        }
    };
    HardwareProfile {
        tier,
        quality,
        max_gan_scale,
    }
}

fn tier_from_memory(vram_mib: u64) -> HardwareTier {
    if vram_mib >= 16_000 {
        HardwareTier::Ultra
    } else if vram_mib >= 8_000 {
        HardwareTier::High
    } else {
        HardwareTier::Balanced
    }
}

#[cfg(windows)]
pub fn detect() -> Result<HardwareProfile, String> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|error| format!("Inspect display adapters: {error}"))?;
    let mut candidates = Vec::new();
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(format!("Enumerate display adapters: {error}")),
        };
        let description = unsafe { adapter.GetDesc1() }
            .map_err(|error| format!("Read display adapter: {error}"))?;
        let end = description
            .Description
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(description.Description.len());
        let name = String::from_utf16_lossy(&description.Description[..end]);
        candidates.push(profile(
            &name,
            description.VendorId,
            description.DedicatedVideoMemory as u64,
            description.Flags & 2 != 0,
        ));
    }
    candidates
        .into_iter()
        .max_by_key(|candidate| candidate.tier)
        .ok_or("DXGI reported no display adapters.".into())
}

#[cfg(target_os = "linux")]
pub fn detect() -> Result<HardwareProfile, String> {
    let adapters = std::fs::read_dir("/sys/class/drm")
        .map_err(|error| format!("Inspect display adapters: {error}"))?;
    let mut candidates = Vec::new();
    for adapter in adapters {
        let adapter = adapter.map_err(|error| format!("Read display adapter: {error}"))?;
        let name = adapter.file_name();
        let Some(index) = name.to_str().and_then(|name| name.strip_prefix("card")) else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let device = adapter.path().join("device");
        let Ok(vendor) = std::fs::read_to_string(device.join("vendor")) else {
            continue;
        };
        let vendor = u32::from_str_radix(vendor.trim().trim_start_matches("0x"), 16)
            .map_err(|error| format!("Read display adapter vendor: {error}"))?;
        let memory = std::fs::read_to_string(device.join("mem_info_vram_total"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0);
        let name = std::fs::read_to_string(device.join("product_name")).unwrap_or_default();
        candidates.push(profile(&name, vendor, memory, false));
    }
    candidates
        .into_iter()
        .max_by_key(|candidate| candidate.tier)
        .ok_or("No DRM display adapter was reported.".into())
}
