#![allow(dead_code)]

//! A tiny self-cleaning temp dir, so tests need no extra crate. Always under the system temp
//! dir, never under `~/.t3`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-launch-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        // Resolve /var → /private/var, so paths compare equal to what tools print.
        TempDir(p.canonicalize().unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A new directory inside this one.
    pub fn dir(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
