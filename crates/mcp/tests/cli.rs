//! The real binary as a CLI, against a temporary `AIRBOSS_DB`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{TempDir, run, stdout};

#[test]
fn help_lists_the_commands() {
    let dir = TempDir::new();
    let out = run(&dir.db(), &["--help"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("mcp"), "{text}");
}

#[test]
fn version_prints_the_crate_version() {
    let dir = TempDir::new();
    let out = run(&dir.db(), &["--version"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out).trim(),
        format!("airboss {}", env!("CARGO_PKG_VERSION"))
    );
}
