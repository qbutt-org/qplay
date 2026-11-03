use super::super::framegen::Backend;

#[derive(Clone, Copy)]
pub enum ArchiveKind {
    File,
    Zip,
    SevenZip,
}
#[derive(Clone, Copy)]
pub enum AssetSource {
    Network {
        url: &'static str,
        bytes: u64,
    },
    Bundled {
        relative_path: &'static str,
        bytes: u64,
    },
}
#[derive(Clone, Copy)]
pub struct Asset {
    pub id: &'static str,
    pub label: &'static str,
    pub filename: &'static str,
    pub source: AssetSource,
    pub sha256: &'static str,
    pub installed_bytes: u64,
    pub kind: ArchiveKind,
}

#[cfg(windows)]
pub const VAPOURSYNTH: Asset = Asset {
    id: "vapoursynth",
    label: "VapourSynth R79",
    filename: "VapourSynth64-Portable-R79.zip",
    source: AssetSource::Network {
        url: "https://github.com/vapoursynth/vapoursynth/releases/download/R79/VapourSynth64-Portable-R79.zip",
        bytes: 23_160_674,
    },
    sha256: "625b3410d903943107291592e90d6f521f829ebb8291d952ee91b8d674bbb153",
    installed_bytes: 10_087_954,
    kind: ArchiveKind::Zip,
};

#[cfg(windows)]
pub const VSTRT: Asset = Asset {
    id: "vstrt",
    label: "vs-mlrt TensorRT plugin",
    filename: "VSTRT-Windows-x64.v15.16.7z",
    source: AssetSource::Network {
        url: "https://github.com/AmusementClub/vs-mlrt/releases/download/v15.16/VSTRT-Windows-x64.v15.16.7z",
        bytes: 486_704,
    },
    sha256: "c4e64e69e87553bf15a7acd29d76debd006954e39ee07ef0e6d526947c5d34b1",
    installed_bytes: 2_292_736,
    kind: ArchiveKind::SevenZip,
};

#[cfg(windows)]
pub const VSOV: Asset = Asset {
    id: "vsov",
    label: "vs-mlrt OpenVINO 2026.2.1 plugin",
    filename: "framegen-vsov.dll",
    source: AssetSource::Bundled {
        relative_path: "profile/tools/bin/framegen-vsov.dll",
        bytes: 4_231_168,
    },
    sha256: "25229f2263d6dfe2794e7140c8cdc6093e40c24f5fc840225b73e974db65cd22",
    installed_bytes: 4_231_168,
    kind: ArchiveKind::File,
};

#[cfg(windows)]
pub const OPENVINO_RUNTIME: Asset = Asset {
    id: "openvino-runtime",
    label: "OpenVINO 2026.2.1 GPU runtime",
    filename: "openvino-2026.2.1-21919-cp313-cp313-win_amd64.whl",
    source: AssetSource::Network {
        url: "https://files.pythonhosted.org/packages/ee/e5/13b9bfc20c02706d015e96e5b3e932227d658b3826b4aa5007cdc2455ed4/openvino-2026.2.1-21919-cp313-cp313-win_amd64.whl",
        bytes: 76_171_396,
    },
    sha256: "197435104d18657a7cc4d05850615b913618ebb0d3baaec052fda25a2d831c74",
    installed_bytes: 66_669_201,
    kind: ArchiveKind::Zip,
};

#[cfg(windows)]
pub const TRT_LIBS: Asset = Asset {
    id: "tensorrt-libs",
    label: "TensorRT 10.16 [CUDA 13]",
    filename: "tensorrt_cu13_libs-10.16.0.72-py3-none-win_amd64.whl",
    source: AssetSource::Network {
        url: "https://pypi.nvidia.com/tensorrt-cu13-libs/tensorrt_cu13_libs-10.16.0.72-py3-none-win_amd64.whl",
        bytes: 1_911_163_963,
    },
    sha256: "11c6703d440f929f057be91e18d6425d657bd6f5f3635cd5a85ea5015e00f310",
    installed_bytes: 1_117_567_024,
    kind: ArchiveKind::Zip,
};

pub const RIFE_REGULAR: Asset = Asset {
    id: "rife-4.26-trt",
    label: "RIFE 4.26",
    filename: "rife_v4.26.7z",
    source: AssetSource::Network {
        url: "https://github.com/AmusementClub/vs-mlrt/releases/download/external-models/rife_v4.26.7z",
        bytes: 19_591_599,
    },
    sha256: "dfdabd84a2a3db773f87604b8cc255e94a6a72f13550d910ccd3b4ee2606cd4f",
    installed_bytes: 22_748_049,
    kind: ArchiveKind::SevenZip,
};

#[cfg(windows)]
pub const RIFE_OPENVINO: Asset = Asset {
    id: "rife-4.26-openvino-s0p5",
    label: "RIFE 4.26 [Intel optimized]",
    filename: "rife_v4.26.onnx",
    source: AssetSource::Bundled {
        relative_path: "profile/framegen/models/openvino/rife_v2/rife_v4.26.onnx",
        bytes: 22_751_073,
    },
    sha256: "3a958289369a5645339b2fbd5f3a228bc82075866c2b4723e4ca485fda72fc6b",
    installed_bytes: 22_751_073,
    kind: ArchiveKind::File,
};

#[cfg(target_os = "linux")]
include!(concat!(env!("OUT_DIR"), "/framegen_assets.rs"));

#[cfg(target_os = "linux")]
pub const TRT_LIBS: Asset = Asset {
    id: "tensorrt-libs",
    label: "TensorRT 10.16 [CUDA 13]",
    filename: "tensorrt_cu13_libs-10.16.0.72-py3-none-manylinux_2_28_x86_64.whl",
    source: AssetSource::Network {
        url: "https://pypi.nvidia.com/tensorrt-cu13-libs/tensorrt_cu13_libs-10.16.0.72-py3-none-manylinux_2_28_x86_64.whl",
        bytes: 3_728_699_633,
    },
    sha256: "11262f57f6dac4ee0c02a3c90b1f10cc609701136815ce3c606f24436277bfee",
    installed_bytes: 1_406_128_120,
    kind: ArchiveKind::Zip,
};

pub fn selected(backend: Backend) -> Result<Vec<Asset>, String> {
    #[cfg(target_os = "linux")]
    {
        if backend != Backend::TensorRt {
            return Err("Native OpenVINO is unavailable on this platform.".into());
        }
        let mut assets = LINUX_NATIVE_FILES.to_vec();
        assets.extend([TRT_LIBS, RIFE_REGULAR]);
        Ok(assets)
    }
    #[cfg(windows)]
    {
        Ok(match backend {
            Backend::TensorRt => vec![VAPOURSYNTH, VSTRT, TRT_LIBS, RIFE_REGULAR],
            Backend::OpenVino => vec![VAPOURSYNTH, VSOV, OPENVINO_RUNTIME, RIFE_OPENVINO],
        })
    }
}

pub fn tensorrt_resources(capability: &str) -> [String; 2] {
    let architecture = capability.replace('.', "");
    if cfg!(windows) {
        [
            "nvinfer_builder_resource_ptx_10.dll".into(),
            format!("nvinfer_builder_resource_sm{architecture}_10.dll"),
        ]
    } else {
        [
            "libnvinfer_builder_resource_ptx.so.10.16.0".into(),
            format!("libnvinfer_builder_resource_sm{architecture}.so.10.16.0"),
        ]
    }
}
