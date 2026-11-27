//! Opening the database: connection settings and per-module migrations.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior, params};

use crate::{Error, Result};

/// One schema change for one module, applied once, in its own transaction. Another module
/// hands its list to `Store::migrate_module` (rule 31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// 1 for the first; each later one is higher. Never reuse or edit a shipped one.
    pub version: i64,
    /// The statements to run, as one batch.
    pub sql: &'static str,
}

impl crate::Store {
    /// Runs another module's pending migrations on this file, each once, in its own
    /// transaction, recorded under `module`. `core` itself, a blank name, or versions that
    /// aren't 1 or more and strictly increasing are `Error::InvalidTarget` (rule 31).
    pub fn migrate_module(&mut self, module: &str, migrations: &[Migration]) -> Result<()> {
        check_module_migrations(module, migrations)?;
        migrate(&mut self.conn, module, migrations)
    }
}

/// Rule 31's checks, made before anything runs.
fn check_module_migrations(module: &str, migrations: &[Migration]) -> Result<()> {
    let ordered = migrations.first().is_none_or(|m| m.version >= 1)
        && migrations.windows(2).all(|w| w[0].version < w[1].version);
    if module == "core" || module.trim().is_empty() || !ordered {
        return Err(Error::InvalidTarget);
    }
    Ok(())
}

/// `core`'s migrations, oldest first. Never edit a shipped one; add a new version.
const CORE_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("migrations/001_core.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/002_pr_refs.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/003_event_ids.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("migrations/004_repo_ids.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("migrations/005_herdr_links.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("migrations/006_priority.sql"),
    },
];

const CREATE_SCHEMA_MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS schema_migrations (
  module      TEXT NOT NULL,
  version     INTEGER NOT NULL,
  applied_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (module, version)
);";

/// Opens a database file, applying settings and `core`'s pending migrations.
pub(crate) fn open_file(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // Wait for other handles before anything else, so switching to WAL and migrating
    // don't fail when two handles open the same file at once.
    conn.busy_timeout(Duration::from_millis(2000))?;
    // WAL must be set outside any transaction; it persists in the file.
    set_wal(&conn)?;
    prepare(conn)
}

/// Switches the file to WAL. While another handle is opening the same new file, SQLite can
/// refuse the switch with "database is locked" without calling the busy handler, so retry for
/// up to the same 2 seconds `busy_timeout` would wait.
fn set_wal(conn: &Connection) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_millis(2000);
    loop {
        match conn.pragma_update(None, "journal_mode", "WAL") {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            other => return Ok(other?),
        }
    }
}

/// Opens a private in-memory database, for tests and throwaway use.
pub(crate) fn open_memory() -> Result<Connection> {
    prepare(Connection::open_in_memory()?)
}

fn prepare(mut conn: Connection) -> Result<Connection> {
    conn.busy_timeout(Duration::from_millis(2000))?;
    // NORMAL is safe with WAL and skips a disk sync on every commit.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn, "core", CORE_MIGRATIONS)?;
    Ok(conn)
}

/// Applies `module`'s migrations newer than the newest one recorded for that module.
///
/// Each migration takes the write lock first (IMMEDIATE) and re-reads the version inside
/// it, so two handles opening one file can't both apply the same migration.
///
/// Around each one it follows SQLite's procedure for schema changes: foreign keys go off
/// first, since a table rebuild would otherwise cascade or fail partway, and `PRAGMA
/// foreign_keys` does nothing inside a transaction (step 1). Before the commit,
/// `foreign_key_check` must find nothing (step 10), and afterwards foreign keys go back
/// to how they were, whether the migration worked or not (step 12).
pub(crate) fn migrate(conn: &mut Connection, module: &str, migrations: &[Migration]) -> Result<()> {
    // Nearly every open finds everything applied. Find that out with a plain read, so opening
    // the file (every CLI command, every agent's `airboss mcp`) never waits for the write lock
    // another process holds, and fails only if it really has a migration to apply.
    let newest = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    let applied = applied_version(conn, module)?;
    // A newer Airboss migrated this file. This binary doesn't know its rules, so it neither
    // reads nor writes it; an older `airboss mcp` once wrote refs the newer one never matched.
    if applied > newest {
        return Err(Error::newer_database());
    }
    if applied == newest {
        return Ok(());
    }
    for m in migrations {
        let enforced: bool = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
        if enforced {
            conn.pragma_update(None, "foreign_keys", "OFF")?;
        }
        let applied = apply(conn, module, m, enforced);
        let restored = match enforced {
            true => conn.pragma_update(None, "foreign_keys", "ON"),
            false => Ok(()),
        };
        applied?;
        restored?;
    }
    // Another handle may have applied a newer one while this one waited for the lock.
    if applied_version(conn, module)? > newest {
        return Err(Error::newer_database());
    }
    Ok(())
}

/// `Error::newer_database` if a newer Airboss has migrated `core` past this binary, read
/// inside the write transaction so nothing is written after that happens.
pub(crate) fn refuse_newer_core(conn: &Connection) -> Result<()> {
    let newest = CORE_MIGRATIONS.last().map_or(0, |m| m.version);
    if applied_version(conn, "core")? > newest {
        return Err(Error::newer_database());
    }
    Ok(())
}

/// The newest version of `module` applied, read without the write lock; 0 before any.
fn applied_version(conn: &Connection, module: &str) -> Result<i64> {
    let has_table: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(0);
    }
    Ok(conn.query_row(
        "SELECT ifnull(max(version), 0) FROM schema_migrations WHERE module = ?1",
        params![module],
        |row| row.get(0),
    )?)
}

/// One migration in its own transaction, unless it's already applied. With `check_keys`,
/// it's refused if it leaves any row breaking a foreign key.
fn apply(conn: &mut Connection, module: &str, m: &Migration, check_keys: bool) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Created inside the write lock: a plain CREATE IF NOT EXISTS reads, then writes,
    // and that upgrade fails at once with "database is locked" when another handle is
    // migrating, without waiting for busy_timeout.
    tx.execute_batch(CREATE_SCHEMA_MIGRATIONS)?;
    let applied: i64 = tx.query_row(
        "SELECT ifnull(max(version), 0) FROM schema_migrations WHERE module = ?1",
        params![module],
        |row| row.get(0),
    )?;
    if m.version <= applied {
        return Ok(());
    }
    tx.execute_batch(m.sql)?;
    if check_keys && tx.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(Error::Db(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            Some(format!(
                "{module} migration {} leaves rows breaking a foreign key",
                m.version
            )),
        )));
    }
    tx.execute(
        "INSERT INTO schema_migrations (module, version) VALUES (?1, ?2)",
        params![module, m.version],
    )?;
    Ok(tx.commit()?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn pragma(conn: &Connection, name: &str) -> String {
        conn.pragma_query_value(None, name, |row| row.get::<_, rusqlite::types::Value>(0))
            .map(|v| match v {
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Text(t) => t,
                other => format!("{other:?}"),
            })
            .unwrap()
    }

    fn temp_db() -> (tempdir::Dir, std::path::PathBuf) {
        let dir = tempdir::Dir::new();
        let path = dir.0.join("airboss.db");
        (dir, path)
    }

    /// A tiny self-cleaning temp dir, so tests need no extra crate.
    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let p = std::env::temp_dir().join(format!(
                    "airboss-core-test-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    /// `core`'s newest migration: every core migration, applied once.
    const CORE_VERSION: i64 = 6;

    fn rows<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> Vec<Vec<Option<T>>> {
        let mut stmt = conn.prepare(sql).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migration_6_adds_a_priority_that_starts_unset_and_takes_only_0_to_3() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&mut conn, "core", &CORE_MIGRATIONS[..5]).unwrap();
        conn.execute_batch(
            "INSERT INTO tasks (title, position, origin) VALUES ('one', 1, 'me'), ('two', 2, 'me');",
        )
        .unwrap();
        let sql = "SELECT id, parent_id, title, body, status, position, origin, created_at FROM tasks ORDER BY id";
        let before = rows::<rusqlite::types::Value>(&conn, sql);

        migrate(&mut conn, "core", CORE_MIGRATIONS).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION);
        assert_eq!(rows(&conn, sql), before, "every task kept as it was");
        assert_eq!(
            rows::<i64>(&conn, "SELECT priority FROM tasks ORDER BY id"),
            vec![vec![None], vec![None]],
            "no task has a priority yet"
        );
        for p in 0..=3 {
            conn.execute("UPDATE tasks SET priority = ?1 WHERE id = 1", [p])
                .unwrap();
        }
        for bad in ["4", "-1", "'P1'"] {
            let set = format!("UPDATE tasks SET priority = {bad} WHERE id = 1");
            assert!(conn.execute(&set, []).is_err(), "{bad} is refused");
        }
    }

    #[test]
    fn migration_5_keeps_every_link_and_takes_herdr_terminals() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&mut conn, "core", &CORE_MIGRATIONS[..4]).unwrap();
        conn.execute_batch(
            "INSERT INTO tasks (title, position, origin) VALUES ('one', 1, 'me'), ('two', 2, 'me');
             INSERT INTO links (id, task_id, kind, ref, url, title, pr_state, linked_by, linked_at) VALUES
               (1, 1, 't3_thread', 'th-1', 't3code://threads/e/th-1', 'Thread', NULL, 'launch', '2026-11-25T10:00:00.000Z'),
               (2, 1, 'claude_session', 'cs-1', NULL, NULL, NULL, 'agent:claude/cs-1', '2026-11-25T10:00:01.000Z'),
               (3, 1, 'pr', 'acme/web#1', 'https://github.com/acme/web/pull/1', NULL, 'open', 'me', '2026-11-25T10:00:02.000Z'),
               (4, 2, 'slack', 'C1/p1', 'https://acme.slack.com/archives/C1/p1', NULL, NULL, 'me', '2026-11-25T10:00:03.000Z'),
               (7, 2, 'doc', '/w/plan.md', NULL, 'Plan', NULL, 'me', '2026-11-25T10:00:04.000Z'),
               (9, 2, 'github_issue', 'acme/web#9', 'https://github.com/acme/web/issues/9', NULL, NULL, 'me', '2026-11-25T10:00:05.000Z');",
        )
        .unwrap();
        let sql = "SELECT id, task_id, kind, ref, url, title, pr_state, linked_by, linked_at FROM links ORDER BY id";
        let before = rows::<rusqlite::types::Value>(&conn, sql);
        assert!(
            conn.execute(
                "INSERT INTO links (task_id, kind, ref, linked_by) VALUES (1, 'herdr_terminal', 't4p', 'launch')",
                [],
            )
            .is_err(),
            "version 4 refuses the new kind"
        );

        migrate(&mut conn, "core", CORE_MIGRATIONS).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION);
        assert_eq!(rows(&conn, sql), before, "every link kept, ids and all");
        conn.execute(
            "INSERT INTO links (task_id, kind, ref, linked_by) VALUES (1, 'herdr_terminal', 't4p', 'launch')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO links (task_id, kind, ref, linked_by) VALUES (1, 'herdr_terminal', 't4p', 'agent:herdr/t4p')",
                [],
            )
            .is_err(),
            "one link per task, kind and ref"
        );
        assert!(
            conn.execute(
                "INSERT INTO links (task_id, kind, ref, linked_by) VALUES (1, 'nonsense', 'x', 'me')",
                [],
            )
            .is_err(),
            "other kinds are still refused"
        );
        let index: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'links_kind_ref_idx'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index, 1, "the kind and ref index is back");
        // Deleting a task still takes its links with it.
        conn.execute("DELETE FROM tasks WHERE id = 2", []).unwrap();
        let left: i64 = conn
            .query_row("SELECT count(*) FROM links WHERE task_id = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn migration_4_keeps_every_repo_and_link_and_never_reuses_a_repo_id() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&mut conn, "core", &CORE_MIGRATIONS[..3]).unwrap();
        conn.execute_batch(
            "INSERT INTO repos (id, name, path, remote_url) VALUES
               (1, 'web', '/w/web', 'git@github.com:acme/web.git'),
               (2, 'api', '/w/api', NULL),
               (5, 'ios', '/w/ios', NULL);
             INSERT INTO tasks (title, position, origin) VALUES ('one', 1, 'me'), ('two', 2, 'me');
             INSERT INTO task_repos (task_id, repo_id, linked_by, linked_at) VALUES
               (1, 1, 'me', '2026-11-25T10:00:00.000Z'),
               (1, 5, 'plan', '2026-11-25T10:00:01.000Z'),
               (2, 2, 'me', '2026-11-25T10:00:02.000Z');",
        )
        .unwrap();
        let repos_sql = "SELECT id, name, path, remote_url FROM repos ORDER BY id";
        let links_sql = "SELECT task_id, repo_id, linked_by, linked_at FROM task_repos ORDER BY task_id, repo_id";
        let (repos_before, links_before) = (
            rows::<rusqlite::types::Value>(&conn, repos_sql),
            rows::<rusqlite::types::Value>(&conn, links_sql),
        );

        migrate(&mut conn, "core", CORE_MIGRATIONS).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION);
        assert_eq!(rows(&conn, repos_sql), repos_before, "every repo kept");
        assert_eq!(rows(&conn, links_sql), links_before, "every link kept");

        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'repos'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("id          INTEGER PRIMARY KEY AUTOINCREMENT"),
            "{sql}"
        );
        let seq: i64 = conn
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'repos'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(seq, 5, "seeded at max(id)");
        assert_eq!(pragma(&conn, "foreign_keys"), "1", "switched back on");
        // task_repos still points at repos, not at the table it was rebuilt from.
        let links: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'task_repos'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(links.contains("REFERENCES repos(id)"), "{links}");

        // Both UNIQUEs, the foreign key and the cascade came across.
        for dup in [
            "INSERT INTO repos (name, path) VALUES ('web', '/elsewhere')",
            "INSERT INTO repos (name, path) VALUES ('other', '/w/web')",
            "INSERT INTO task_repos (task_id, repo_id, linked_by) VALUES (2, 99, 'me')",
            "DELETE FROM repos WHERE id = 1",
        ] {
            assert!(conn.execute(dup, []).is_err(), "{dup}");
        }
        conn.execute("DELETE FROM tasks WHERE id = 1", []).unwrap();
        assert_eq!(
            rows::<i64>(&conn, "SELECT task_id FROM task_repos"),
            vec![vec![Some(2)]]
        );

        // The newest repo removed, the next one gets a new id.
        conn.execute("DELETE FROM repos WHERE id = 5", []).unwrap();
        conn.execute(
            "INSERT INTO repos (name, path) VALUES ('android', '/w/android')",
            [],
        )
        .unwrap();
        assert_eq!(conn.last_insert_rowid(), 6);
    }

    type EventRow = (
        i64,
        i64,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    );

    fn event_rows(conn: &Connection) -> Vec<EventRow> {
        conn.prepare(
            "SELECT id, task_id, kind, to_status, body, actor, at FROM task_events ORDER BY id",
        )
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn migration_3_keeps_every_event_and_never_reuses_an_id() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&mut conn, "core", &CORE_MIGRATIONS[..2]).unwrap();
        conn.execute_batch(
            "INSERT INTO tasks (title, position, origin, status, blocked_reason)
               VALUES ('one', 1, 'me', 'todo', NULL), ('two', 2, 'me', 'blocked', 'why');
             INSERT INTO task_events (id, task_id, kind, to_status, body, actor, at) VALUES
               (1, 1, 'created', NULL, NULL, 'me', '2026-11-25T10:00:00.000Z'),
               (2, 2, 'created', NULL, 'waits on #1', 'plan', '2026-11-25T10:00:01.000Z'),
               (4, 2, 'status', 'blocked', 'why', 'agent:t3/a', '2026-11-25T10:00:02.000Z'),
               (7, 1, 'note', NULL, 'a note', 'sync:gh', '2026-11-25T10:00:03.000Z'),
               (9, 1, 'repo_linked', NULL, 'web', 'me', '2026-11-25T10:00:04.000Z');",
        )
        .unwrap();
        let before = event_rows(&conn);

        migrate(&mut conn, "core", CORE_MIGRATIONS).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION);
        assert_eq!(event_rows(&conn), before, "every row and column kept");

        let sql = |name: &str| -> String {
            conn.query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [name],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(sql("task_events").contains("id         INTEGER PRIMARY KEY AUTOINCREMENT"));
        assert!(sql("task_events_task_idx").contains("task_events(task_id, id)"));
        let seq: i64 = conn
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 'task_events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(seq, 9);
        assert_eq!(pragma(&conn, "foreign_keys"), "1", "switched back on");

        // Deleting the newest event's task doesn't free its id.
        conn.execute_batch(
            "DELETE FROM tasks WHERE id = 1;
             INSERT INTO task_events (task_id, kind, actor) VALUES (2, 'note', 'me');",
        )
        .unwrap_err(); // a note needs a body: the CHECKs came across
        conn.execute(
            "INSERT INTO task_events (task_id, kind, body, actor) VALUES (2, 'note', 'n', 'me')",
            [],
        )
        .unwrap();
        assert_eq!(conn.last_insert_rowid(), 10);
        // The cascade from tasks and the foreign key to tasks came across too.
        assert!(event_rows(&conn).iter().all(|e| e.1 == 2));
        assert!(
            conn.execute(
                "INSERT INTO task_events (task_id, kind, actor) VALUES (99, 'created', 'me')",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn migration_2_puts_stored_pr_refs_in_one_form() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&mut conn, "core", &CORE_MIGRATIONS[..1]).unwrap();
        conn.execute_batch(
            "INSERT INTO tasks (title, position, origin) VALUES ('one', 1, 'me'), ('two', 2, 'me'),
                                                                ('three', 3, 'me');
             INSERT INTO links (task_id, kind, ref, pr_state, linked_by) VALUES
               (1, 'pr', 'Acme/Web#1', 'open', 'me'),
               (2, 'pr', 'Acme/Web#2', 'merged', 'me'),
               (2, 'pr', 'acme/web#2', 'open', 'me'),
               (3, 'pr', 'https://github.com/A/B/pull/3', 'open', 'me');
             INSERT INTO links (task_id, kind, ref, linked_by) VALUES (1, 'doc', 'Notes.md', 'me');",
        )
        .unwrap();
        migrate(&mut conn, "core", CORE_MIGRATIONS).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION);

        let rows: Vec<(i64, String, Option<String>)> = conn
            .prepare("SELECT task_id, ref, pr_state FROM links ORDER BY task_id, id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let open = Some("open".to_string());
        assert_eq!(
            rows,
            vec![
                (1, "acme/web#1".into(), open.clone()),
                (1, "Notes.md".into(), None),
                // The lowercase twin stays, with its own state.
                (2, "acme/web#2".into(), open.clone()),
                // Never valid; left as it was.
                (3, "https://github.com/A/B/pull/3".into(), open),
            ]
        );
    }

    fn core_version(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT max(version) FROM schema_migrations WHERE module = 'core'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_database_a_newer_airboss_migrated_is_refused_and_left_alone() {
        let (_dir, path) = temp_db();
        {
            let conn = open_file(&path).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (module, version) VALUES ('core', ?1)",
                [CORE_VERSION + 1],
            )
            .unwrap();
        }
        let err = open_file(&path).unwrap_err();
        assert!(err.is_newer_database(), "{err}");
        assert_eq!(
            err.to_string(),
            "This database was created by a newer Airboss. Update the `airboss` command."
        );
        let conn = Connection::open(&path).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION + 1, "nothing undone");
    }

    #[test]
    fn a_module_newer_than_this_binary_knows_is_refused() {
        let mut conn = open_memory().unwrap();
        let both = [
            Migration {
                version: 1,
                sql: "CREATE TABLE m_a (x INTEGER);",
            },
            Migration {
                version: 2,
                sql: "CREATE TABLE m_b (x INTEGER);",
            },
        ];
        migrate(&mut conn, "sync", &both).unwrap();
        migrate(&mut conn, "sync", &both).unwrap();
        let err = migrate(&mut conn, "sync", &both[..1]).unwrap_err();
        assert!(err.is_newer_database(), "{err}");
    }

    #[test]
    fn file_database_uses_wal_normal_sync_and_foreign_keys() {
        let (_dir, path) = temp_db();
        let conn = open_file(&path).unwrap();
        assert_eq!(pragma(&conn, "journal_mode"), "wal");
        assert_eq!(pragma(&conn, "synchronous"), "1"); // NORMAL
        assert_eq!(pragma(&conn, "foreign_keys"), "1");
        assert_eq!(pragma(&conn, "busy_timeout"), "2000");
    }

    #[test]
    fn new_database_gets_every_core_table_and_index() {
        let conn = open_memory().unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','index') AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        for expected in [
            "links",
            "links_kind_ref_idx",
            "repos",
            "schema_migrations",
            "task_deps",
            "task_deps_waits_on_idx",
            "task_events",
            "task_events_task_idx",
            "task_repos",
            "task_repos_repo_idx",
            "tasks",
            "tasks_parent_position_idx",
            "tasks_plan_key_idx",
        ] {
            assert!(names.iter().any(|n| n == expected), "missing {expected}");
        }
        assert_eq!(core_version(&conn), CORE_VERSION);
    }

    #[test]
    fn reopening_runs_no_migration_twice_and_keeps_data() {
        let (_dir, path) = temp_db();
        {
            let conn = open_file(&path).unwrap();
            conn.execute("INSERT INTO repos (name, path) VALUES ('web', '/w')", [])
                .unwrap();
        }
        let conn = open_file(&path).unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE module = 'core'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, CORE_VERSION);
        let repos: i64 = conn
            .query_row("SELECT count(*) FROM repos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(repos, 1);
    }

    #[test]
    fn another_modules_migrations_do_not_affect_core() {
        let (_dir, path) = temp_db();
        {
            let mut conn = open_file(&path).unwrap();
            let sync = [
                Migration {
                    version: 1,
                    sql: "CREATE TABLE sync_a (x INTEGER);",
                },
                Migration {
                    version: 2,
                    sql: "CREATE TABLE sync_b (x INTEGER);",
                },
                Migration {
                    version: 3,
                    sql: "CREATE TABLE sync_c (x INTEGER);",
                },
            ];
            migrate(&mut conn, "sync", &sync).unwrap();
        }
        let conn = open_file(&path).unwrap();
        assert_eq!(core_version(&conn), CORE_VERSION);
        let sync_rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE module = 'sync'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sync_rows, 3);
    }

    #[test]
    fn a_failing_migration_leaves_nothing_behind() {
        let mut conn = open_memory().unwrap();
        let bad = [Migration {
            version: 1,
            sql: "CREATE TABLE half_done (x INTEGER); THIS IS NOT SQL;",
        }];
        assert!(migrate(&mut conn, "broken", &bad).is_err());
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'half_done'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let recorded: i64 = conn
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE module = 'broken'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((tables, recorded), (0, 0));
    }

    #[test]
    fn many_handles_opening_one_new_file_at_once_all_succeed() {
        let (_dir, path) = temp_db();
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let p = path.clone();
                std::thread::spawn(move || open_file(&p).map(|_| ()))
            })
            .collect();
        for h in handles {
            h.join().unwrap().unwrap();
        }
        let conn = open_file(&path).unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE module = 'core'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, CORE_VERSION);
    }

    /// The 10 constraint checks from the spec's schema test, in Rust.
    mod constraints {
        #![allow(clippy::unwrap_used)]
        use super::super::open_memory;
        use rusqlite::Connection;

        fn db() -> Connection {
            let c = open_memory().unwrap();
            c.execute_batch(
                "INSERT INTO tasks (title, position, origin) VALUES ('Booking flow', 1, 'me');
                 INSERT INTO tasks (parent_id, title, position, origin, plan_key) VALUES (1, 'Phase 1', 1, 'plan', 'p1');
                 INSERT INTO links (task_id, kind, ref, pr_state, linked_by) VALUES (2, 'pr', 'o/r#42', 'open', 'me');
                 INSERT INTO repos (name, path) VALUES ('web', '/w/web');
                 INSERT INTO task_repos (task_id, repo_id, linked_by) VALUES (2, 1, 'me');",
            )
            .unwrap();
            c
        }

        fn rejects(sql: &str) {
            let c = db();
            assert!(c.execute_batch(sql).is_err(), "should be rejected: {sql}");
        }

        #[test]
        fn blank_title() {
            rejects("INSERT INTO tasks (title, position, origin) VALUES ('  ', 2, 'me')");
        }
        #[test]
        fn position_below_one() {
            rejects("INSERT INTO tasks (title, position, origin) VALUES ('x', 0, 'me')");
        }
        #[test]
        fn plan_key_on_non_plan_task() {
            rejects(
                "INSERT INTO tasks (title, position, origin, plan_key) VALUES ('x', 2, 'me', 'k')",
            );
        }
        #[test]
        fn same_plan_key_twice_under_one_parent() {
            rejects(
                "INSERT INTO tasks (parent_id, title, position, origin, plan_key) VALUES (1, 'again', 2, 'plan', 'p1')",
            );
        }
        #[test]
        fn same_plan_key_twice_at_top_level() {
            rejects(
                "INSERT INTO tasks (title, position, origin, plan_key) VALUES ('a', 2, 'plan', 'top');
                 INSERT INTO tasks (title, position, origin, plan_key) VALUES ('b', 3, 'plan', 'top');",
            );
        }
        #[test]
        fn pr_link_without_state() {
            rejects(
                "INSERT INTO links (task_id, kind, ref, linked_by) VALUES (2, 'pr', 'o/r#43', 'me')",
            );
        }
        #[test]
        fn state_on_non_pr_link() {
            rejects(
                "INSERT INTO links (task_id, kind, ref, pr_state, linked_by) VALUES (2, 'doc', 'a.md', 'open', 'me')",
            );
        }
        #[test]
        fn note_event_without_text() {
            rejects("INSERT INTO task_events (task_id, kind, actor) VALUES (2, 'note', 'me')");
        }
        #[test]
        fn unknown_event_kind() {
            rejects("INSERT INTO task_events (task_id, kind, actor) VALUES (2, 'renamed', 'me')");
        }
        #[test]
        fn same_migration_twice() {
            rejects("INSERT INTO schema_migrations (module, version) VALUES ('core', 1)");
        }
        #[test]
        fn blocked_without_reason() {
            rejects("UPDATE tasks SET status = 'blocked' WHERE id = 1");
        }
        #[test]
        fn task_that_is_its_own_parent() {
            rejects("UPDATE tasks SET parent_id = 1 WHERE id = 1");
        }
        #[test]
        fn status_event_without_status() {
            rejects("INSERT INTO task_events (task_id, kind, actor) VALUES (2, 'status', 'me')");
        }
        #[test]
        fn same_link_twice() {
            rejects(
                "INSERT INTO links (task_id, kind, ref, pr_state, linked_by) VALUES (2, 'pr', 'o/r#42', 'open', 'me')",
            );
        }
        #[test]
        fn same_repo_twice_on_one_task() {
            rejects("INSERT INTO task_repos (task_id, repo_id, linked_by) VALUES (2, 1, 'me')");
        }
        #[test]
        fn repo_link_to_a_missing_repo() {
            rejects("INSERT INTO task_repos (task_id, repo_id, linked_by) VALUES (1, 9, 'me')");
        }
        #[test]
        fn deleting_a_task_removes_its_subtree_links_and_events() {
            let c = db();
            c.execute_batch(
                "INSERT INTO task_events (task_id, kind, actor) VALUES (2, 'created', 'me');
                 DELETE FROM tasks WHERE id = 1;",
            )
            .unwrap();
            let left: i64 = c
                .query_row(
                    "SELECT (SELECT count(*) FROM tasks) + (SELECT count(*) FROM links)
                          + (SELECT count(*) FROM task_events) + (SELECT count(*) FROM task_repos)",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(left, 0);
        }
    }
}
