use std::path::PathBuf;

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let lock_path = PathBuf::from("../../upstream-lock.json");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&lock_path).expect("read upstream-lock.json"))
            .expect("parse upstream-lock.json");
    let target = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    let hash = match target.as_str() {
        "windows" => &lock["runtime"]["files"]["libmpv-2.dll"],
        "linux" => &lock["linux"]["runtime"]["files"]["libmpv.so.2"],
        _ => panic!("Unsupported qplay target: {target}"),
    }
    .as_str()
    .expect("pinned platform libmpv SHA-256");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        output.join("engine_pin.rs"),
        format!("pub const ENGINE_SHA256: &str = {hash:?};\n"),
    )
    .expect("write engine pin");

    if target == "linux" {
        let files = lock["linux"]["nativeFramegenFiles"]
            .as_object()
            .expect("pinned Linux native frame-generation files");
        let mut assets = String::from("pub const LINUX_NATIVE_FILES: &[Asset] = &[\n");
        for (path, size) in files {
            let size = size.as_u64().expect("native file size");
            assert!(size > 0, "native file size must be positive");
            let hash = lock["linux"]["runtime"]["files"][path]
                .as_str()
                .expect("native file SHA-256 in the engine inventory");
            let filename = path.rsplit('/').next().expect("native file name");
            assets.push_str(&format!(
                r#"    Asset {{
        id: "bundled-native",
        label: {filename:?},
        filename: {filename:?},
        source: AssetSource::Bundled {{ relative_path: {path:?}, bytes: {size} }},
        sha256: {hash:?},
        installed_bytes: {size},
        kind: ArchiveKind::File,
    }},
"#
            ));
        }
        assets.push_str("];\n");
        std::fs::write(output.join("framegen_assets.rs"), assets).expect("write native asset pins");
    }

    let builder = CxxQtBuilder::new_qml_module(QmlModule::new("Qplay.Player"))
        .qt_module("Qml")
        .files([
            "src/controller.rs",
            "src/track_model.rs",
            "src/framegen_graph.rs",
            "src/tensorrt.rs",
        ])
        .cpp_file("src/native.cpp")
        .cpp_file("src/framegen_graph.cpp")
        .cpp_file("src/tensorrt.cpp")
        .crate_include_root(Some("include".into()));
    // SAFETY: Match CMake's UTF-8 source and execution character sets.
    unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("/utf-8");
            cc.flag_if_supported("/EHsc");
            cc.include("vendor/tensorrt");
            println!("cargo:rerun-if-env-changed=CUDA_PATH");
            if let Some(cuda) = std::env::var_os("CUDA_PATH") {
                let include = PathBuf::from(cuda).join("include");
                if include.join("cuda_runtime_api.h").is_file() {
                    cc.include(include);
                    cc.define("QPLAY_TENSORRT", None);
                }
            }
        })
    }
    .build();
}
