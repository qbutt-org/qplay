use super::super::framegen::Backend;

use std::{fs, path::Path};

#[derive(Clone, Copy)]
struct RuntimeNotice {
    filename: &'static str,
    contents: &'static [u8],
}

const COMMON: &[RuntimeNotice] = &[
    RuntimeNotice {
        filename: "THIRD-PARTY-NOTICES.md",
        contents: include_bytes!("../../../../../licenses/framegen/THIRD-PARTY-NOTICES.md"),
    },
    RuntimeNotice {
        filename: "vs-mlrt-GPL-3.0.txt",
        contents: include_bytes!("../../../../../licenses/framegen/vs-mlrt-GPL-3.0.txt"),
    },
    RuntimeNotice {
        filename: "Practical-RIFE-MIT.txt",
        contents: include_bytes!("../../../../../licenses/framegen/Practical-RIFE-MIT.txt"),
    },
    RuntimeNotice {
        filename: "vs-rife-MIT.txt",
        contents: include_bytes!("../../../../../licenses/framegen/vs-rife-MIT.txt"),
    },
];

const TENSORRT_OSS: &[RuntimeNotice] = &[
    RuntimeNotice {
        filename: "TensorRT-OSS-LICENSE.txt",
        contents: include_bytes!("../../../../../licenses/framegen/TensorRT-OSS-LICENSE.txt"),
    },
    RuntimeNotice {
        filename: "TensorRT-OSS-NOTICE.txt",
        contents: include_bytes!("../../../../../licenses/framegen/TensorRT-OSS-NOTICE.txt"),
    },
];

const OPENVINO: &[RuntimeNotice] = &[
    RuntimeNotice {
        filename: "RIFE-Intel-PROVENANCE.json",
        contents: include_bytes!("../../../../../profile/framegen/models/openvino/PROVENANCE.json"),
    },
    RuntimeNotice {
        filename: "RIFE-Intel-REBUILD.md",
        contents: include_bytes!("../../../../../profile/tools/framegen-models/README.md"),
    },
    RuntimeNotice {
        filename: "VSOV-PROVENANCE.json",
        contents: include_bytes!("../../../../../profile/tools/framegen-vsov/PROVENANCE.json"),
    },
    RuntimeNotice {
        filename: "VSOV-BUILDING.md",
        contents: include_bytes!("../../../../../profile/tools/framegen-vsov/BUILDING.md"),
    },
    RuntimeNotice {
        filename: "VSOV-MODIFICATIONS.md",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/source/MODIFICATIONS.md"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-SOURCE-DOWNLOADS.json",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/source/SOURCE-DOWNLOADS.json"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-VapourSynth-R54-LGPL-2.1.txt",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/licenses/VapourSynth-R54-LGPL-2.1.txt"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-VapourSynth-R54-VSHelper-WTFPL-2.0.txt",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/licenses/VapourSynth-R54-VSHelper-WTFPL-2.0.txt"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-ONNX-Apache-2.0.txt",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/licenses/onnx-Apache-2.0.txt"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-Protobuf-BSD-3-Clause.txt",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/licenses/protobuf-BSD-3-Clause.txt"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-Abseil-Apache-2.0.txt",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/licenses/abseil-Apache-2.0.txt"
        ),
    },
    RuntimeNotice {
        filename: "VSOV-utf8-range-MIT.txt",
        contents: include_bytes!(
            "../../../../../profile/tools/framegen-vsov/licenses/utf8-range-MIT.txt"
        ),
    },
    RuntimeNotice {
        filename: "OpenVINO-Apache-2.0.txt",
        contents: include_bytes!("../../../../../licenses/framegen/OpenVINO-Apache-2.0.txt"),
    },
    RuntimeNotice {
        filename: "OpenVINO-third-party-programs.txt",
        contents: include_bytes!(
            "../../../../../licenses/framegen/OpenVINO-third-party-programs.txt"
        ),
    },
    RuntimeNotice {
        filename: "OpenVINO-oneTBB-third-party-programs.txt",
        contents: include_bytes!(
            "../../../../../licenses/framegen/OpenVINO-oneTBB-third-party-programs.txt"
        ),
    },
    RuntimeNotice {
        filename: "OpenVINO-oneDNN-third-party-programs.txt",
        contents: include_bytes!(
            "../../../../../licenses/framegen/OpenVINO-oneDNN-third-party-programs.txt"
        ),
    },
];

pub(crate) fn write(root: &Path, backend: Backend) -> std::io::Result<()> {
    let notices = root.join("notices");
    fs::create_dir_all(&notices)?;
    let backend_notices = match backend {
        Backend::TensorRt => TENSORRT_OSS,
        Backend::OpenVino => OPENVINO,
    };
    for notice in COMMON.iter().chain(backend_notices) {
        fs::write(notices.join(notice.filename), notice.contents)?;
    }
    Ok(())
}
