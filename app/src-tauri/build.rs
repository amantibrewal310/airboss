//! Tauri's build step, with the app's command list for the capability files.

use std::path::Path;

include!("src/command_names.rs");

fn main() {
    // `bundle.externalBin` must exist for tauri-build to copy it. A real build puts the release
    // `airboss` there first (scripts/build-sidecar.sh); a plain `cargo build` or `cargo test`
    // gets an empty stand-in, which is never bundled.
    let sidecar = Path::new("binaries/airboss-aarch64-apple-darwin");
    if !sidecar.exists()
        && let Err(e) =
            std::fs::create_dir_all("binaries").and_then(|()| std::fs::write(sidecar, b""))
    {
        println!("cargo:warning=couldn't write the stand-in sidecar: {e}");
    }
    println!("cargo:rerun-if-changed=src/command_names.rs");
    println!("cargo:rerun-if-changed=capabilities");
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMAND_NAMES));
    if let Err(e) = tauri_build::try_build(attributes) {
        println!("cargo:warning={e:#}");
        std::process::exit(1);
    }
}
