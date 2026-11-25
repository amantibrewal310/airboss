//! Launch's own tables: every CHECK, the one-pending index, and applying once.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/temp.rs"]
mod temp;

use airboss_core::Store;
use airboss_launch::MIGRATIONS;
use rusqlite::{Connection, params};
use temp::TempDir;

fn open(dir: &TempDir) -> Connection {
    let path = dir.path().join("airboss.db");
    let mut store = Store::open(&path).unwrap();
    store.migrate_module("launch", MIGRATIONS).unwrap();
    Connection::open(&path).unwrap()
}

/// Inserts a row from its columns; `Err` when SQLite refuses it.
#[allow(clippy::too_many_arguments)]
fn insert(
    c: &Connection,
    task: i64,
    repo: Option<i64>,
    mode: &str,
    state: &str,
    thread: Option<&str>,
    branch: Option<&str>,
    base: Option<&str>,
    linked: i64,
    error: Option<&str>,
) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT INTO launch_threads
           (task_id, repo_id, mode, state, thread_id, project_path, branch, base_ref, title,
            linked, error)
         VALUES (?1, ?2, ?3, ?4, ?5, '/r', ?6, ?7, 't', ?8, ?9)",
        params![task, repo, mode, state, thread, branch, base, linked, error],
    )
}

#[test]
fn valid_rows_go_in() {
    let dir = TempDir::new();
    let c = open(&dir);
    let b = Some("task-1-x");
    let m = Some("main");
    insert(&c, 1, Some(1), "do", "pending", None, b, m, 0, None).unwrap();
    insert(&c, 1, Some(2), "do", "started", Some("t1"), b, m, 1, None).unwrap();
    insert(&c, 1, Some(3), "do", "failed", None, b, m, 0, Some("no")).unwrap();
    insert(&c, 1, Some(4), "do", "lost", None, b, m, 0, None).unwrap();
    insert(&c, 1, None, "plan", "pending", None, None, None, 0, None).unwrap();
    insert(&c, 1, None, "do", "pending", None, None, None, 0, None).unwrap();
}

#[test]
fn each_check_rejects_its_bad_row() {
    let dir = TempDir::new();
    let c = open(&dir);
    let b = Some("task-1-x");
    let m = Some("main");
    let bad = [
        // mode and state values
        insert(&c, 1, Some(1), "doit", "pending", None, b, m, 0, None),
        insert(&c, 1, Some(1), "do", "running", None, b, m, 0, None),
        // started ⇔ thread id
        insert(&c, 1, Some(1), "do", "started", None, b, m, 0, None),
        insert(&c, 1, Some(1), "do", "pending", Some("t9"), b, m, 0, None),
        // linked only when started
        insert(&c, 1, Some(1), "do", "pending", None, b, m, 1, None),
        insert(&c, 1, Some(1), "do", "started", Some("t8"), b, m, 2, None),
        // failed ⇔ error
        insert(&c, 1, Some(1), "do", "failed", None, b, m, 0, None),
        insert(&c, 1, Some(1), "do", "lost", None, b, m, 0, Some("x")),
        // branch ⇔ base
        insert(&c, 1, Some(1), "do", "pending", None, b, None, 0, None),
        insert(&c, 1, Some(1), "do", "pending", None, None, m, 0, None),
        // a branch only for Do it in a repo
        insert(&c, 1, Some(1), "plan", "pending", None, b, m, 0, None),
        insert(&c, 1, None, "do", "pending", None, b, m, 0, None),
    ];
    for (i, r) in bad.iter().enumerate() {
        assert!(r.is_err(), "bad row {i} went in");
    }
    let n: i64 = c
        .query_row("SELECT count(*) FROM launch_threads", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn one_pending_row_per_task_repo_and_mode() {
    let dir = TempDir::new();
    let c = open(&dir);
    let b = Some("task-1-x");
    let m = Some("main");
    insert(&c, 1, Some(1), "do", "pending", None, b, m, 0, None).unwrap();
    assert!(insert(&c, 1, Some(1), "do", "pending", None, b, m, 0, None).is_err());
    // Another repo, mode or task is fine.
    insert(&c, 1, Some(2), "do", "pending", None, b, m, 0, None).unwrap();
    insert(&c, 1, Some(1), "plan", "pending", None, None, None, 0, None).unwrap();
    insert(&c, 2, Some(1), "do", "pending", None, b, m, 0, None).unwrap();
    // A null repo counts as one place too.
    insert(&c, 3, None, "do", "pending", None, None, None, 0, None).unwrap();
    assert!(insert(&c, 3, None, "do", "pending", None, None, None, 0, None).is_err());
    // Once it settles, a new pending row may go in.
    c.execute(
        "UPDATE launch_threads SET state = 'lost' WHERE task_id = 1 AND repo_id = 1 AND mode = 'do'",
        [],
    )
    .unwrap();
    insert(&c, 1, Some(1), "do", "pending", None, b, m, 0, None).unwrap();
}

#[test]
fn thread_ids_are_unique() {
    let dir = TempDir::new();
    let c = open(&dir);
    let b = Some("task-1-x");
    let m = Some("main");
    insert(&c, 1, Some(1), "do", "started", Some("t1"), b, m, 0, None).unwrap();
    assert!(insert(&c, 2, Some(1), "do", "started", Some("t1"), b, m, 0, None).is_err());
}

#[test]
fn launch_repos_refuses_an_empty_base() {
    let dir = TempDir::new();
    let c = open(&dir);
    c.execute(
        "INSERT INTO launch_repos (repo_id, base_ref) VALUES (1, 'develop')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO launch_repos (repo_id, base_ref) VALUES (2, NULL)",
        [],
    )
    .unwrap();
    assert!(
        c.execute(
            "INSERT INTO launch_repos (repo_id, base_ref) VALUES (3, '')",
            []
        )
        .is_err()
    );
}

#[test]
fn opening_twice_applies_once() {
    let dir = TempDir::new();
    let path = dir.path().join("airboss.db");
    for _ in 0..2 {
        let mut store = Store::open(&path).unwrap();
        store.migrate_module("launch", MIGRATIONS).unwrap();
    }
    let c = Connection::open(&path).unwrap();
    let applied: i64 = c
        .query_row(
            "SELECT count(*) FROM schema_migrations WHERE module = 'launch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(applied, MIGRATIONS.len() as i64);
}

#[test]
fn version_2_adds_the_repo_path_and_keeps_every_launch_repos_row() {
    let dir = TempDir::new();
    let path = dir.path().join("airboss.db");
    let mut store = Store::open(&path).unwrap();
    store.migrate_module("launch", &MIGRATIONS[..1]).unwrap();
    let c = Connection::open(&path).unwrap();
    c.execute(
        "INSERT INTO launch_repos (repo_id, base_ref, t3_project_id) VALUES (4, 'develop', 'p4')",
        [],
    )
    .unwrap();
    store.migrate_module("launch", MIGRATIONS).unwrap();
    let row: (i64, Option<String>, Option<String>, Option<String>) = c
        .query_row(
            "SELECT repo_id, base_ref, t3_project_id, repo_path FROM launch_repos",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    // Kept, with no path: launch won't trust it until it records the repo again.
    assert_eq!(row, (4, Some("develop".into()), Some("p4".into()), None));
    assert!(
        c.execute(
            "INSERT INTO launch_repos (repo_id, repo_path) VALUES (5, '')",
            []
        )
        .is_err(),
        "an empty path is refused"
    );
}
