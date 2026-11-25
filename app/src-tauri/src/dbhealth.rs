//! Noticing a database that broke while the app runs (integration QA R2).
//!
//! Two cheap signs, and nothing is repaired:
//! - **A read or write fails** with an I/O or corruption error from SQLite
//!   ([`airboss_core::Error::is_storage_failure`]), seen in [`AppState::core`].
//! - **The `-wal` file vanishes or is replaced** while the app holds it: one `stat` on each
//!   catch-up. The app's connection keeps the WAL open, so SQLite never deletes it while the
//!   app runs. Without it, new connections (the CLI, agents) fail with "disk I/O error", and
//!   the app's own writes would be lost when it quits.
//!
//! Either way the app is in the database problem state for the rest of the run: one
//! `db:unavailable` event, and every command but the few the state itself needs fails with
//! `db_unavailable`. Quitting and opening Airboss again is the fix.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tauri_plugin_log::log;

use crate::errors::{AppError, ErrorCode};
use crate::shell::UiEvent;
use crate::state::{AppState, lock};

/// What the full-window state says: what to do.
pub const MSG_LOST: &str = "Quit Airboss (⌘Q) and open it again. Changes made since the problem began may not have been saved. Troubleshooting has more.";

/// Commands that still run in the database problem state: Show in Finder, and what shows
/// and hides the windows.
pub const WHILE_BROKEN: &[&str] = &[
    "open_url",
    "ui_ready",
    "show_main",
    "set_focus_context",
    "quickadd_shown",
    "quickadd_hide",
    "quickadd_resize",
];

/// The database's health as the app last saw it.
#[derive(Debug, Default)]
pub(crate) struct DbHealth {
    db: Option<PathBuf>,
    wal: Option<PathBuf>,
    /// The WAL's device and inode, from the first time it was seen.
    wal_id: Option<(u64, u64)>,
    broken: Option<AppError>,
}

impl DbHealth {
    /// For the database at `db`; `None` for an in-memory store, which has no WAL.
    pub fn new(db: Option<PathBuf>) -> DbHealth {
        let wal = db.as_deref().map(crate::watcher::wal_path);
        let mut h = DbHealth {
            db,
            wal,
            ..DbHealth::default()
        };
        h.wal_id = h.wal.as_deref().and_then(file_id);
        h
    }

    /// What's wrong with the WAL now, if anything: it was there and is gone or replaced.
    fn wal_problem(&mut self) -> Option<String> {
        let wal = self.wal.as_deref()?;
        let name = wal.file_name()?.to_string_lossy().into_owned();
        let now = file_id(wal);
        match (self.wal_id, now) {
            (None, now) => {
                self.wal_id = now;
                None
            }
            (Some(_), None) => Some(format!(
                "The write-ahead log ({name}) was deleted while Airboss had the database open."
            )),
            (Some(was), Some(now)) if was != now => Some(format!(
                "The write-ahead log ({name}) was replaced while Airboss had the database open."
            )),
            _ => None,
        }
    }

    /// The error every command answers with, `"<path>: <reason>"` as its detail.
    fn error(&self, reason: &str) -> AppError {
        let path = self
            .db
            .as_deref()
            .map_or_else(|| "the database".to_string(), |p| p.display().to_string());
        AppError::new(ErrorCode::DbUnavailable, MSG_LOST).with_detail(format!("{path}: {reason}"))
    }
}

fn file_id(p: &Path) -> Option<(u64, u64)> {
    std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

impl AppState {
    /// The database problem, once there is one.
    pub fn db_problem(&self) -> Option<AppError> {
        lock(&self.db).broken.clone()
    }

    /// One `stat` of the WAL; on the first sign of trouble, the database problem state.
    pub(crate) fn check_db_files(&self) {
        let reason = {
            let mut h = lock(&self.db);
            if h.broken.is_some() {
                return;
            }
            h.wal_problem()
        };
        if let Some(reason) = reason {
            self.db_broke(&reason);
        }
    }

    /// A `core` call failed; if SQLite says the file itself is broken, that's the problem state.
    pub(crate) fn saw_core_error(&self, e: &airboss_core::Error) {
        if e.is_storage_failure() {
            self.db_broke(&e.to_string());
        }
    }

    /// Enters the database problem state, once: logged, and `db:unavailable` sent.
    fn db_broke(&self, reason: &str) {
        let err = {
            let mut h = lock(&self.db);
            if h.broken.is_some() {
                return;
            }
            let err = h.error(reason);
            h.broken = Some(err.clone());
            err
        };
        log::error!(
            "database problem: {}",
            err.detail.as_deref().unwrap_or(reason)
        );
        self.shell.emit(UiEvent::DbUnavailable(err));
    }
}
