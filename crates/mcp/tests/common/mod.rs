//! Helpers shared by the integration tests. Each test file uses only some of them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-mcp-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn db(&self) -> PathBuf {
        self.0.join("airboss.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The `airboss` binary, with a clean environment: only `AIRBOSS_DB` pointing at `db`, and
/// none of the thread variables this test process may have inherited.
pub fn airboss(db: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_airboss"));
    for var in [
        "T3CODE_THREAD_ID",
        "CLAUDE_CODE_SESSION_ID",
        "AIRBOSS_THREAD",
        "NO_COLOR",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("AIRBOSS_DB", db);
    cmd
}

/// Runs the CLI with `args` and returns what it did.
pub fn run(db: &Path, args: &[&str]) -> Output {
    airboss(db).args(args).output().unwrap()
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}
