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
const CORE_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("migrations/001_core.sql"),
}];

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
pub(crate) fn migrate(conn: &mut Connection, module: &str, migrations: &[Migration]) -> Result<()> {
    for m in migrations {
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
            continue;
        }
        tx.execute_batch(m.sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (module, version) VALUES (?1, ?2)",
            params![module, m.version],
        )?;
        tx.commit()?;
    }
    Ok(())
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

    fn core_version(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT max(version) FROM schema_migrations WHERE module = 'core'",
            [],
            |r| r.get(0),
        )
        .unwrap()
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
        assert_eq!(core_version(&conn), 1);
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
        assert_eq!(rows, 1);
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
        assert_eq!(core_version(&conn), 1);
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
        assert_eq!(rows, 1);
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
