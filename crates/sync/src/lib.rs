//! `airboss-sync`: live state from GitHub, T3 and Claude Code, inside the app process.
//!
//! See `SPEC-sync.md` at the repo root. Two worker threads (T3 and Claude; GitHub) and the
//! kqueue watcher's thread; no tokio, no daemon.

// The crate is built bottom up; this goes once `Sync` uses every layer (task T8).
#![allow(dead_code)]

pub mod claude;
mod db;
pub mod gh;
mod health;
mod hub;
mod secret;
pub mod t3;
pub mod threads;
mod worker;

pub use db::{MIGRATIONS, MODULE, SyncDb};
pub use hub::Hub;
pub use secret::Secret;
pub use worker::{
    Env, GhMsg, MAX_WAIT, Step, StoreSummary, T3Msg, T3Worker, WalWatch, WatchTarget,
};

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where `sync` reads from, and how often.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConfig {
    /// `~/.t3/userdata`: `statev2.sqlite`, its `-wal`, `environment-id`.
    pub t3_dir: PathBuf,
    /// `~/.claude/sessions`.
    pub claude_sessions: PathBuf,
    /// `None`: look in `/opt/homebrew/bin`, `/usr/local/bin`, then `PATH`.
    pub gh_path: Option<PathBuf>,
    /// The GitHub timer: 5 minutes.
    pub pr_every: Duration,
    /// The least time between two T3 passes: 2 s.
    pub t3_min_gap: Duration,
    /// How far back untracked threads go: 7 days.
    pub untracked_window: Duration,
}

impl SyncConfig {
    /// The standard places under `home`, and the spec's timings.
    pub fn defaults(home: &Path) -> SyncConfig {
        SyncConfig {
            t3_dir: home.join(".t3").join("userdata"),
            claude_sessions: home.join(".claude").join("sessions"),
            gh_path: None,
            pr_every: Duration::from_secs(5 * 60),
            t3_min_gap: Duration::from_secs(2),
            untracked_window: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

#[cfg(feature = "fakes")]
pub mod fake;
