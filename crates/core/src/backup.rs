//! `airboss backup`: a consistent copy of the database file, while everything keeps running.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use rusqlite::backup::{Backup, StepResult};

use crate::{Error, Result};

/// How long to keep trying while SQLite says the copy is busy.
const BUSY_FOR: Duration = Duration::from_secs(2);

impl crate::Store {
    /// Writes a copy of the whole database to `to`, every module's tables included, through
    /// SQLite's online backup: one read of one moment, so writes from this or any other
    /// process before or after it don't tear it, and nobody waits on it.
    ///
    /// An existing file at `to` is `Error::AlreadyExists` unless `overwrite`, and the
    /// database's own file is always `Error::InvalidTarget`. The copy is written next to
    /// `to` first and then renamed over it, so a failed backup leaves `to` as it was.
    pub fn backup(&self, to: &Path, overwrite: bool) -> Result<()> {
        if self.path().is_some_and(|own| same_file(&own, to)) {
            return Err(Error::InvalidTarget);
        }
        if !overwrite && to.symlink_metadata().is_ok() {
            return Err(Error::AlreadyExists);
        }
        let temp = temp_path(to);
        let copied = copy(&self.conn, &temp).and_then(|()| {
            if overwrite {
                // An older copy's leftovers would be read as part of the new one.
                for side in ["-wal", "-shm"] {
                    let _ = std::fs::remove_file(with_suffix(to, side));
                }
            }
            std::fs::rename(&temp, to).map_err(|e| {
                Error::cant_open(format!("couldn't put the backup at {}: {e}", to.display()))
            })
        });
        if copied.is_err() {
            for side in ["", "-journal", "-wal", "-shm"] {
                let _ = std::fs::remove_file(with_suffix(&temp, side));
            }
        }
        copied
    }
}

/// Every page in one step, so the copy is a single moment.
fn copy(from: &Connection, temp: &Path) -> Result<()> {
    let mut dst = Connection::open(temp)?;
    {
        let backup = Backup::new(from, &mut dst)?;
        let deadline = Instant::now() + BUSY_FOR;
        loop {
            match backup.step(-1)? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                StepResult::Busy | StepResult::Locked => {
                    return Err(Error::Db(rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                        None,
                    )));
                }
                // `StepResult` may grow; treat anything new as "try again".
                _ => {}
            }
        }
    }
    dst.close().map_err(|(_, e)| Error::Db(e))
}

/// Whether two paths name one file, even spelled differently or not yet created.
fn same_file(a: &Path, b: &Path) -> bool {
    let full = |p: &Path| -> Option<PathBuf> {
        p.canonicalize().ok().or_else(|| {
            let dir = p.parent().filter(|d| !d.as_os_str().is_empty())?;
            Some(dir.canonicalize().ok()?.join(p.file_name()?))
        })
    };
    match (full(a), full(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// A hidden name in `to`'s folder, so the rename stays on one volume.
fn temp_path(to: &Path) -> PathBuf {
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "airboss.db".into());
    to.with_file_name(format!(".{name}.airboss-backup-{}", std::process::id()))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}
