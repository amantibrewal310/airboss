//! Tauri's build step, with the app's command list for the capability files.

include!("src/command_names.rs");

fn main() {
    // No sidecar here: `bundle.externalBin` lives in `tauri.bundle.conf.json`, which only the
    // release build passes (`pnpm bundle`). tauri-build copies a configured sidecar next to
    // the binary it builds, which used to put an empty stand-in over `target/debug/airboss`.
    println!("cargo:rerun-if-changed=src/command_names.rs");
    println!("cargo:rerun-if-changed=capabilities");
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMAND_NAMES));
    if let Err(e) = tauri_build::try_build(attributes) {
        println!("cargo:warning={e:#}");
        std::process::exit(1);
    }
}
