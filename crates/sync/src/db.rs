//! `sync`'s own tables, on its own connection to the app's database file.
//!
//! The migration runs through `Store::migrate_module("sync", …)`, so `core` records it; the
//! tables are then read and written on a connection of our own, opened at `Store::path()`.
//! `core`'s tables are never touched here.

use std::path::Path;
use std::sync::PoisonError;
use std::time::{Duration, SystemTime};

use airboss_core::Migration;
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{ErrorCode, ServiceError, SharedStore};
use rusqlite::{Connection, params};

use crate::health::{Health, HealthState};

/// `sync`'s migrations, oldest first. Version 1 hasn't shipped, so it may still be edited.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("migrations/001_sync.sql"),
}];

/// The module name the migrations are recorded under.
pub const MODULE: &str = "sync";

/// One connection to `sync`'s tables.
pub struct SyncDb {
    pub(crate) conn: Connection,
}

impl SyncDb {
    /// Runs `sync`'s migrations on the shared store's file, then opens a connection of our
    /// own on it. An in-memory store is refused: there's no file to open.
    pub fn open(store: &SharedStore) -> Result<SyncDb, ServiceError> {
        let path = {
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            store
                .migrate_module(MODULE, MIGRATIONS)
                .map_err(|e| storage(format!("sync's tables couldn't be set up: {e}")))?;
            store.path().ok_or_else(|| {
                storage("sync needs the database file; this store is in memory".to_string())
            })?
        };
        SyncDb::open_path(&path)
    }

    /// Our own connection on a file whose migrations already ran.
    pub(crate) fn open_path(path: &Path) -> Result<SyncDb, ServiceError> {
        let conn = Connection::open(path)
            .map_err(|e| storage(format!("sync couldn't open {}: {e}", path.display())))?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|e| storage(e.to_string()))?;
        Ok(SyncDb { conn })
    }
}

/// What a failed statement on our own tables becomes. Our tables are ours alone, so this only
/// happens when the disk or the file fails.
pub(crate) type DbResult<T> = rusqlite::Result<T>;

impl SyncDb {
    /// Every `sync_health` row: `(source, scope, health)`. Rows with text we can't read are
    /// skipped; the next pass rewrites them.
    pub(crate) fn load_health(&self) -> DbResult<Vec<(String, String, Health)>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, scope, state, detail, since, last_ok_at, failures, next_try_at
             FROM sync_health ORDER BY source, scope",
        )?;
        let rows = stmt.query_map([], |r| {
            let state: String = r.get(2)?;
            let since: String = r.get(4)?;
            let last_ok: Option<String> = r.get(5)?;
            let next: Option<String> = r.get(7)?;
            let failures: i64 = r.get(6)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                HealthState::parse(&state).and_then(|state| {
                    Some(Health {
                        state,
                        detail: r.get(3).ok()?,
                        since: from_ts(&since)?,
                        last_ok_at: last_ok.as_deref().and_then(from_ts),
                        failures: u32::try_from(failures).unwrap_or(0),
                        next_try_at: next.as_deref().and_then(from_ts),
                        stale: true,
                    })
                }),
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (source, scope, health) = row?;
            if let Some(health) = health {
                out.push((source, scope, health));
            }
        }
        Ok(out)
    }

    /// Writes one source's health.
    pub(crate) fn save_health(&self, source: &str, scope: &str, h: &Health) -> DbResult<()> {
        let ts = |t: Option<SystemTime>| t.map(to_ts);
        self.conn.execute(
            "INSERT INTO sync_health (source, scope, state, detail, since, last_ok_at, failures, next_try_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (source, scope) DO UPDATE SET state = ?3, detail = ?4, since = ?5,
               last_ok_at = ?6, failures = ?7, next_try_at = ?8",
            params![
                source,
                scope,
                h.state.as_str(),
                h.detail,
                to_ts(h.since),
                ts(h.last_ok_at),
                h.failures,
                ts(h.next_try_at)
            ],
        )?;
        Ok(())
    }

    /// Forgets one source's health, e.g. a `gh` account that logged out for good.
    pub(crate) fn delete_health(&self, source: &str, scope: &str) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM sync_health WHERE source = ?1 AND scope = ?2",
            params![source, scope],
        )?;
        Ok(())
    }
}

pub(crate) fn storage(message: String) -> ServiceError {
    ServiceError::new(ErrorCode::Storage, message)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn db() -> SyncDb {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0].sql).unwrap();
        SyncDb { conn }
    }

    #[test]
    fn health_round_trips() {
        let db = db();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_000_000);
        let mut h = Health::unknown(t0);
        h.fail("no network", t0);
        db.save_health("gh", "me", &h).unwrap();
        h.ok(t0 + Duration::from_secs(60));
        db.save_health("t3", "", &h).unwrap();
        let rows = db.load_health().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "gh");
        assert_eq!(rows[0].2.state, HealthState::Broken);
        assert_eq!(rows[0].2.detail.as_deref(), Some("no network"));
        assert_eq!(rows[1].2.last_ok_at, Some(t0 + Duration::from_secs(60)));
        assert!(rows[1].2.stale, "loaded rows are stale until the next pass");
        db.delete_health("gh", "me").unwrap();
        assert_eq!(db.load_health().unwrap().len(), 1);
    }
}
