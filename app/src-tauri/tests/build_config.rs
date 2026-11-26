//! The final audit's 0-byte CLI: tauri-build copies `bundle.externalBin` next to the binary it
//! builds, so with the sidecar in the main config every `cargo build` or `cargo test` of the
//! app copied the empty stand-in over `target/debug/airboss`, and the CLI tests failed. The
//! sidecar now lives in a config only the release build passes (`--config`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

fn json(name: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

#[test]
fn only_the_release_build_bundles_the_sidecar() {
    let main = json("tauri.conf.json");
    assert!(
        main["bundle"].get("externalBin").is_none(),
        "tauri-build would copy the sidecar over target/debug/airboss"
    );
    let bundle = json("tauri.bundle.conf.json");
    assert_eq!(
        bundle["bundle"]["externalBin"],
        serde_json::json!(["binaries/airboss"])
    );
    // tauri-build merges a platform file by itself, which would bring the bug back.
    for merged in [
        "tauri.macos.conf.json",
        "tauri.macos.conf.json5",
        "Tauri.macos.toml",
    ] {
        assert!(
            !Path::new(env!("CARGO_MANIFEST_DIR")).join(merged).exists(),
            "{merged}"
        );
    }
}

#[test]
fn the_build_script_writes_no_stand_in() {
    let build =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs")).unwrap();
    assert!(
        !build.contains("binaries/"),
        "build.rs mustn't write a sidecar"
    );
}

#[test]
fn the_release_build_command_passes_the_bundle_config() {
    let pkg = json("../package.json");
    let script = pkg["scripts"]["bundle"].as_str().unwrap();
    assert!(
        script.contains("tauri build --config src-tauri/tauri.bundle.conf.json"),
        "{script}"
    );
}
