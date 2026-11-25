//! `airboss-sync`: live state from GitHub, T3 and Claude Code, inside the app process.
//!
//! See `SPEC-sync.md` at the repo root. Two worker threads (T3 and Claude; GitHub) and the
//! kqueue watcher's thread; no tokio, no daemon.

// The crate is built bottom up; this goes once `Sync` uses every layer (task T8).
#![allow(dead_code)]

mod db;
mod health;
mod secret;
pub mod t3;

pub use db::{MIGRATIONS, MODULE, SyncDb};
pub use secret::Secret;

#[cfg(feature = "fakes")]
pub mod fake;
