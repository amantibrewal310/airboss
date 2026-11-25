//! `attention` migration 1: each CHECK, and opening twice applies it once.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_attention::HiddenStore;
use common::*;
use rusqlite::Connection;

fn opened() -> (TempDir, Connection) {
    let dir = TempDir::new();
    // The app's store creates the file first, as it does at launch.
    airboss_core::Store::open(&dir.db()).unwrap();
    HiddenStore::open(&dir.db()).unwrap();
    let conn = Connection::open(dir.db()).unwrap();
    (dir, conn)
}

fn insert(conn: &Connection, key: &str, kind: &str, action: &str, until: Option<&str>) -> bool {
    conn.execute(
        "INSERT INTO attention_hidden (item_key, task_id, kind, action, until)
         VALUES (?1, 1, ?2, ?3, ?4)",
        rusqlite::params![key, kind, action, until],
    )
    .is_ok()
}

#[test]
fn a_good_row_of_every_kind_goes_in() {
    let (_dir, conn) = opened();
    for kind in [
        "question",
        "approval",
        "review",
        "failed_run",
        "proposed",
        "ready",
        "finish",
        "quiet",
        "stopped",
    ] {
        assert!(
            insert(&conn, &format!("{kind}:1"), kind, "dismissed", None),
            "{kind}"
        );
    }
    assert!(insert(
        &conn,
        "quiet:2",
        "quiet",
        "snoozed",
        Some("2026-11-29T00:00:00.000Z")
    ));
    let hidden_at: String = conn
        .query_row(
            "SELECT hidden_at FROM attention_hidden WHERE item_key = 'quiet:2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hidden_at.len(), 24, "{hidden_at}");
}

#[test]
fn each_check_rejects_its_bad_row() {
    let (_dir, conn) = opened();
    let until = Some("2026-11-29T00:00:00.000Z");
    assert!(!insert(&conn, "", "quiet", "dismissed", None), "empty key");
    assert!(
        !insert(&conn, "x:1", "untracked", "dismissed", None),
        "unknown kind"
    );
    assert!(
        !insert(&conn, "x:2", "quiet", "muted", None),
        "unknown action"
    );
    assert!(
        !insert(&conn, "x:3", "quiet", "snoozed", None),
        "snooze without until"
    );
    assert!(
        !insert(&conn, "x:4", "quiet", "dismissed", until),
        "dismissal with until"
    );
    let n: i64 = conn
        .query_row("SELECT count(*) FROM attention_hidden", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn opening_twice_applies_the_migration_once() {
    let (dir, conn) = opened();
    HiddenStore::open(&dir.db()).unwrap();
    let applied: Vec<(String, i64)> = conn
        .prepare("SELECT module, version FROM schema_migrations WHERE module = 'attention'")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        applied,
        vec![("attention".to_string(), 1), ("attention".to_string(), 2)]
    );
    // core still opens the same file.
    airboss_core::Store::open(&dir.db()).unwrap();
}

#[test]
fn a_new_file_gets_cores_tables_and_ours() {
    let dir = TempDir::new();
    HiddenStore::open(&dir.db()).unwrap();
    let store = airboss_core::Store::open(&dir.db()).unwrap();
    assert!(store.snapshot().unwrap().tree.is_empty());
}

#[test]
fn version_two_keeps_every_row_from_version_one() {
    let dir = TempDir::new();
    // A file at attention v1, as 26 Nov 2026 shipped it, with one row.
    let mut store = airboss_core::Store::open(&dir.db()).unwrap();
    store
        .migrate_module(
            "attention",
            &[airboss_core::Migration {
                version: 1,
                sql: include_str!("../src/migrations/001_attention.sql"),
            }],
        )
        .unwrap();
    drop(store);
    let conn = Connection::open(dir.db()).unwrap();
    assert!(insert(
        &conn,
        "quiet:9",
        "quiet",
        "snoozed",
        Some("2026-11-29T00:00:00.000Z")
    ));
    assert!(!insert(&conn, "stopped:1", "stopped", "dismissed", None));
    drop(conn);

    let hidden = HiddenStore::open(&dir.db()).unwrap();
    let rows = hidden.load().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, key("quiet:9"));
    assert!(rows[0].until.is_some());
    let conn = Connection::open(dir.db()).unwrap();
    assert!(insert(&conn, "stopped:1", "stopped", "dismissed", None));
    let indexed: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'attention_hidden_task_idx'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(indexed, 1);
}
