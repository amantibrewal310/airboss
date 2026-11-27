//! The store handle itself: its file, other connections to it, and other modules' tables.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Error, Migration, Store};
use common::{TempDir, store};

/// A plain connection to the same file, to look at tables `core` doesn't expose.
fn raw(s: &Store) -> rusqlite::Connection {
    rusqlite::Connection::open(s.path().unwrap()).unwrap()
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn recorded(conn: &rusqlite::Connection, module: &str) -> Vec<i64> {
    let mut stmt = conn
        .prepare("SELECT version FROM schema_migrations WHERE module = ?1 ORDER BY version")
        .unwrap();
    stmt.query_map([module], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn file_store(dir: &TempDir) -> Store {
    Store::open(&dir.0.join("airboss.db")).unwrap()
}

const SYNC_V1: Migration = Migration {
    version: 1,
    sql: "CREATE TABLE sync_prs (ref TEXT PRIMARY KEY); INSERT INTO sync_prs VALUES ('a/b#1');",
};
const SYNC_V2: Migration = Migration {
    version: 2,
    sql: "ALTER TABLE sync_prs ADD COLUMN checked_at TEXT;",
};

#[test]
fn a_file_store_knows_its_absolute_path_and_memory_has_none() {
    let dir = TempDir::new();
    let file = dir.0.join("airboss.db");
    let s = Store::open(&file).unwrap();
    let path = s.path().unwrap();
    assert!(path.is_absolute(), "{path:?}");
    assert_eq!(path.canonicalize().unwrap(), file.canonicalize().unwrap());
    assert_eq!(store().path(), None);
}

#[test]
fn a_second_store_opened_on_the_path_sees_the_same_data() {
    let dir = TempDir::new();
    let mut s = Store::open(&dir.0.join("airboss.db")).unwrap();
    s.add_task(
        airboss_core::NewTask::new("Shared"),
        &airboss_core::Actor::Me,
    )
    .unwrap();
    let other = Store::open(&s.path().unwrap()).unwrap();
    assert_eq!(other.tree().unwrap().len(), 1);
}

// Rule 31: another module migrates only its own tables.

#[test]
fn a_modules_migrations_run_once_each_across_handles() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    s.migrate_module("sync", &[SYNC_V1]).unwrap();
    // A second run would fail on CREATE TABLE if it applied version 1 again.
    s.migrate_module("sync", &[SYNC_V1]).unwrap();

    let mut other = Store::open(&s.path().unwrap()).unwrap();
    other.migrate_module("sync", &[SYNC_V1, SYNC_V2]).unwrap();

    let conn = raw(&s);
    assert_eq!(recorded(&conn, "sync"), vec![1, 2]);
    assert_eq!(
        recorded(&conn, "core"),
        vec![1, 2, 3, 4, 5, 6],
        "core's own are untouched"
    );
    assert_eq!(count(&conn, "SELECT count(*) FROM sync_prs"), 1);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM pragma_table_info('sync_prs') WHERE name = 'checked_at'"
        ),
        1
    );
}

#[test]
fn migrating_core_from_outside_is_refused() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    let sneaky = Migration {
        version: 99,
        sql: "DROP TABLE tasks;",
    };
    let err = s.migrate_module("core", &[sneaky]).unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    let conn = raw(&s);
    assert_eq!(recorded(&conn, "core"), vec![1, 2, 3, 4, 5, 6]);
    assert!(s.tree().is_ok(), "tasks is still there");
}

#[test]
fn a_blank_module_or_versions_out_of_order_are_refused_before_anything_runs() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    let v = |version| Migration {
        version,
        sql: "CREATE TABLE should_not_exist (x INTEGER);",
    };
    let cases: Vec<(&str, Vec<Migration>)> = vec![
        ("", vec![v(1)]),
        ("  ", vec![v(1)]),
        ("sync", vec![v(0)]),
        ("sync", vec![v(-1)]),
        ("sync", vec![v(2), v(1)]),
        ("sync", vec![v(1), v(1)]),
    ];
    for (module, migrations) in cases {
        let err = s.migrate_module(module, &migrations).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{module:?}: {err:?}");
    }
    let conn = raw(&s);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM sqlite_master WHERE name = 'should_not_exist'"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM schema_migrations WHERE module <> 'core'"
        ),
        0
    );
}

#[test]
fn a_failing_migration_leaves_nothing_of_itself_and_keeps_the_ones_before() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    let broken = Migration {
        version: 2,
        sql: "CREATE TABLE half_done (x INTEGER); THIS IS NOT SQL;",
    };
    assert!(s.migrate_module("sync", &[SYNC_V1, broken]).is_err());
    let conn = raw(&s);
    assert_eq!(recorded(&conn, "sync"), vec![1]);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM sqlite_master WHERE name = 'half_done'"
        ),
        0
    );
    // Fixed, it goes through, and version 1 isn't run again.
    s.migrate_module("sync", &[SYNC_V1, SYNC_V2]).unwrap();
    assert_eq!(recorded(&conn, "sync"), vec![1, 2]);
}

#[test]
fn migrating_writes_no_events_and_tells_nobody() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    let rx = s.subscribe();
    s.migrate_module("sync", &[SYNC_V1]).unwrap();
    assert!(rx.try_recv().is_err());
    assert_eq!(s.latest_event_id().unwrap(), airboss_core::EventId(0));
}

// Rule 23: writes from another connection show up in `data_version`.

#[test]
fn data_version_changes_when_another_handle_commits_and_not_for_its_own() {
    use airboss_core::{Actor, NewTask};
    let dir = TempDir::new();
    let mut app = file_store(&dir);
    let mut mcp = Store::open(&app.path().unwrap()).unwrap();

    let start = app.data_version().unwrap();
    app.add_task(NewTask::new("Mine"), &Actor::Me).unwrap();
    assert_eq!(
        app.data_version().unwrap(),
        start,
        "own commits don't count"
    );

    let mcp_before = mcp.data_version().unwrap();
    mcp.add_task(NewTask::new("From mcp"), &Actor::Me).unwrap();
    let seen = app.data_version().unwrap();
    assert_ne!(seen, start, "the other handle's commit shows");
    assert_eq!(mcp.data_version().unwrap(), mcp_before);

    // Reading again with nothing new committed gives the same value.
    assert_eq!(app.data_version().unwrap(), seen);
    // A refused write commits nothing, so nothing changes.
    assert!(mcp.add_task(NewTask::new(" "), &Actor::Me).is_err());
    assert_eq!(app.data_version().unwrap(), seen);
}

#[test]
fn data_version_works_in_memory() {
    let s = store();
    let v = s.data_version().unwrap();
    assert_eq!(s.data_version().unwrap(), v);
}

#[test]
fn a_migration_that_breaks_a_foreign_key_is_rolled_back() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    let dangling = Migration {
        version: 1,
        sql: "CREATE TABLE sync_threads (task_id INTEGER NOT NULL REFERENCES tasks(id));
              INSERT INTO sync_threads VALUES (999);",
    };
    assert!(s.migrate_module("sync", &[dangling]).is_err());
    let conn = raw(&s);
    assert!(recorded(&conn, "sync").is_empty());
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM sqlite_master WHERE name = 'sync_threads'"
        ),
        0
    );
    // Foreign keys are back on for the handle's own writes afterwards.
    s.add_task(airboss_core::NewTask::new("x"), &airboss_core::Actor::Me)
        .unwrap();
    assert!(
        s.add_task(
            airboss_core::NewTask::new("orphan").under(airboss_core::TaskId(99)),
            &airboss_core::Actor::Me
        )
        .is_err()
    );
}

// A newer Airboss migrated the file past what this binary knows.

#[test]
fn an_open_store_refuses_writes_once_a_newer_airboss_migrated_the_file() {
    let dir = TempDir::new();
    let mut s = file_store(&dir);
    s.add_task(
        airboss_core::NewTask::new("Before"),
        &airboss_core::Actor::Me,
    )
    .unwrap();
    raw(&s)
        .execute(
            "INSERT INTO schema_migrations (module, version) VALUES ('core', 99)",
            [],
        )
        .unwrap();
    let err = s
        .add_task(
            airboss_core::NewTask::new("After"),
            &airboss_core::Actor::Me,
        )
        .unwrap_err();
    assert!(err.is_newer_database(), "{err}");
    assert_eq!(
        s.tree().unwrap().len(),
        1,
        "reads work; nothing was written"
    );
    match Store::open(&s.path().unwrap()) {
        Err(e) => assert!(e.is_newer_database(), "{e}"),
        Ok(_) => panic!("a newer database opened"),
    }
}

#[test]
fn other_errors_are_not_the_newer_database_error() {
    assert!(!Error::NotFound.is_newer_database());
    let busy = Error::Db(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        None,
    ));
    assert!(!busy.is_newer_database());
}
