use std::path::PathBuf;

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let lock_path = PathBuf::from("../../upstream-lock.json");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&lock_path).expect("read upstream-lock.json"))
            .expect("parse upstream-lock.json");
    let hash = lock["runtime"]["files"]["libmpv-2.dll"]
        .as_str()
        .expect("pin libmpv-2.dll");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        output.join("engine_pin.rs"),
        format!("pub const ENGINE_SHA256: &str = {hash:?};\n"),
    )
    .expect("write engine pin");

    let builder = CxxQtBuilder::new_qml_module(QmlModule::new("Qplay.Player"))
        .qt_module("Qml")
        .files(["src/controller.rs", "src/track_model.rs"])
        .cpp_file("src/native.cpp")
        .crate_include_root(Some("include".into()));
    // SAFETY: Match CMake's UTF-8 source and execution character sets.
    unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("/utf-8");
        })
    }
    .build();
}
