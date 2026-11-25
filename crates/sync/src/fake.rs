//! Fakes for tests, behind the `fakes` feature: every outside source `sync` reads, a
//! settable clock (re-exported from `airboss-services`), and a self-cleaning temp dir.
//! Nothing here runs `gh`, `t3` or `ps`, or opens a real T3 or Claude file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub use airboss_services::fakes::FakeClock;

/// A temp dir removed on drop, so tests need no extra crate.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// A new, empty dir under the system temp dir.
    #[allow(clippy::expect_used)]
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "airboss-sync-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temp dir");
        // Canonical, so paths compare equal to what the kernel and SQLite report (/private/var…).
        TempDir(std::fs::canonicalize(&path).unwrap_or(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Default for TempDir {
    fn default() -> TempDir {
        TempDir::new()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
