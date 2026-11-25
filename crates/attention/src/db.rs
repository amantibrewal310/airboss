//! The worker's own connection to the database file, and the `attention` migration.

use std::path::Path;
use std::time::Duration;

use airboss_core::{Migration, Store};
use rusqlite::Connection;

/// `attention`'s migrations, oldest first. Never edit a shipped one; add a new version.
pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("migrations/001_attention.sql"),
}];

/// Why the table couldn't be opened, for the log.
pub(crate) type OpenError = String;

/// Brings `attention`'s tables up to date through `core`'s runner, on a `Store` of its own,
/// then opens a plain connection for them. `core`'s own tables are never touched.
pub(crate) fn open_file(path: &Path) -> Result<Connection, OpenError> {
    let mut store = Store::open(path).map_err(|e| format!("opening {}: {e}", path.display()))?;
    store
        .migrate_module("attention", MIGRATIONS)
        .map_err(|e| format!("migrating attention: {e}"))?;
    drop(store);
    let conn = Connection::open(path).map_err(|e| format!("opening {}: {e}", path.display()))?;
    // Wait for the app's writes rather than failing on "database is locked", as core does.
    conn.busy_timeout(Duration::from_millis(2000))
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// A private in-memory table, for a `core` store with no file (tests only).
pub(crate) fn open_memory() -> Result<Connection, OpenError> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    for m in MIGRATIONS {
        conn.execute_batch(m.sql).map_err(|e| e.to_string())?;
    }
    Ok(conn)
}
