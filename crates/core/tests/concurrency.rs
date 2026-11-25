//! Several handles on one database file, as `app`, `mcp` and `sync` may have.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, NewTask, Store};
use common::TempDir;

#[test]
fn four_handles_writing_at_once_all_succeed_with_gap_free_positions() {
    let dir = TempDir::new();
    let path = dir.0.join("airboss.db");
    Store::open(&path).unwrap();

    let writers: Vec<_> = (0..4)
        .map(|w| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                for i in 0..50 {
                    store
                        .add_task(NewTask::new(format!("writer {w} task {i}")), &Actor::Me)
                        .unwrap();
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }

    let store = Store::open(&path).unwrap();
    let positions: Vec<u32> = store
        .tree()
        .unwrap()
        .iter()
        .map(|n| n.task.position)
        .collect();
    assert_eq!(positions, (1..=200).collect::<Vec<u32>>());
}

#[test]
fn a_snapshot_sees_one_moment_while_another_handle_writes() {
    let dir = TempDir::new();
    let path = dir.0.join("airboss.db");
    let reader = Store::open(&path).unwrap();

    // Each write adds one task and its one `created` event, so at any committed moment the
    // number of tasks equals the newest event id.
    let writer = {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut store = Store::open(&path).unwrap();
            for i in 0..400 {
                store
                    .add_task(NewTask::new(format!("task {i}")), &Actor::Me)
                    .unwrap();
            }
        })
    };
    let mut seen = 0;
    while !writer.is_finished() {
        let snap = reader.snapshot().unwrap();
        assert_eq!(
            snap.tree.len() as i64,
            snap.latest_event_id.0,
            "tree and latest event id from different moments"
        );
        assert_eq!(snap.ready.len(), snap.tree.len());
        seen += 1;
    }
    writer.join().unwrap();
    assert!(seen > 1, "the reader ran alongside the writer");
}

#[test]
fn opening_a_migrated_database_needs_no_write_lock() {
    // Every CLI command and every agent's `airboss mcp` opens the file. They must open it,
    // and read, while another process holds a long write transaction: in WAL, readers don't
    // wait for writers.
    use std::time::{Duration, Instant};
    let dir = TempDir::new();
    let path = dir.0.join("airboss.db");
    const SQL: &str = "CREATE TABLE qa_things (id INTEGER PRIMARY KEY);";
    let migrations = [airboss_core::Migration {
        version: 1,
        sql: SQL,
    }];
    {
        let mut store = Store::open(&path).unwrap();
        store.add_task(NewTask::new("one"), &Actor::Me).unwrap();
        store.migrate_module("qa", &migrations).unwrap();
    }
    let holder = rusqlite::Connection::open(&path).unwrap();
    holder
        .execute_batch("BEGIN IMMEDIATE; UPDATE tasks SET title = title;")
        .unwrap();

    let started = Instant::now();
    let mut store = Store::open(&path).unwrap();
    store.migrate_module("qa", &migrations).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "waited {:?} for the write lock",
        started.elapsed()
    );
    assert_eq!(store.tree().unwrap().len(), 1);
    holder.execute_batch("COMMIT").unwrap();
}
