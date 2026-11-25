#!/bin/sh
# Builds the `airboss` CLI in release and puts it where `bundle.externalBin` expects it.
# Run by `beforeBuildCommand`; safe to run by hand.
set -eu
cd "$(dirname "$0")/../../.."
cargo build -p airboss-mcp --release
mkdir -p app/src-tauri/binaries
install -m 755 target/release/airboss app/src-tauri/binaries/airboss-aarch64-apple-darwin
