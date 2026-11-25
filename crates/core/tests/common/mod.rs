//! Helpers shared by the integration tests. Each test file uses only some of them.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use airboss_core::{Actor, Store};

pub fn store() -> Store {
    Store::open_in_memory().unwrap()
}

pub fn agent() -> Actor {
    Actor::Agent("t3/abc".into())
}

/// Waits long enough for SQLite's millisecond clock to move on, so tests that compare
/// event times can tell consecutive events apart.
pub fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(2));
}

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A tiny deterministic linear congruential generator, so tests need no `rand`.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
