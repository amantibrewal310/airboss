//! The `sync` migration: each CHECK, and opening twice.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use airboss_core::Store;
use airboss_services::{ErrorCode, SharedStore};
use airboss_sync::SyncDb;
use airboss_sync::fake::TempDir;
use rusqlite::Connection;

fn store_in(dir: &TempDir) -> SharedStore {
    Arc::new(Mutex::new(
        Store::open(&dir.path().join("airboss.db")).unwrap(),
    ))
}

fn conn(dir: &TempDir) -> Connection {
    let store = store_in(dir);
    SyncDb::open(&store).unwrap();
    Connection::open(dir.path().join("airboss.db")).unwrap()
}

fn ok(c: &Connection, sql: &str) {
    c.execute_batch(sql)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn refused(c: &Connection, sql: &str) {
    let err = c.execute_batch(sql).expect_err(sql);
    assert!(err.to_string().contains("CHECK"), "{sql}: {err}");
}

#[test]
fn opening_twice_applies_the_migration_once() {
    let dir = TempDir::new();
    let store = store_in(&dir);
    SyncDb::open(&store).unwrap();
    SyncDb::open(&store).unwrap();
    let c = Connection::open(dir.path().join("airboss.db")).unwrap();
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM schema_migrations WHERE module = 'sync'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn an_in_memory_store_is_refused() {
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
    let err = SyncDb::open(&store).err().unwrap();
    assert_eq!(err.code, ErrorCode::Storage);
}

#[test]
fn observed_threads_checks() {
    let dir = TempDir::new();
    let c = conn(&dir);
    ok(
        &c,
        "INSERT INTO observed_threads (source, ref, linked, state) VALUES ('t3','a',1,'running')",
    );
    ok(
        &c,
        "INSERT INTO observed_threads (source, ref, linked, state, request_id, request_kind)
         VALUES ('claude','b',0,'needs_me','r1','question')",
    );
    for bad in [
        "INSERT INTO observed_threads (source, ref, linked, state) VALUES ('slack','c',1,'idle')",
        "INSERT INTO observed_threads (source, ref, linked, state) VALUES ('t3','',1,'idle')",
        "INSERT INTO observed_threads (source, ref, linked, state) VALUES ('t3','c',2,'idle')",
        "INSERT INTO observed_threads (source, ref, linked, state) VALUES ('t3','c',1,'busy')",
        "INSERT INTO observed_threads (source, ref, linked, state, child_count) VALUES ('t3','c',1,'idle',-1)",
        "INSERT INTO observed_threads (source, ref, linked, state, request_id, request_kind)
         VALUES ('t3','c',1,'needs_me','r','other')",
        "INSERT INTO observed_threads (source, ref, linked, state, pr_seen) VALUES ('t3','c',1,'idle','[')",
        "INSERT INTO observed_threads (source, ref, linked, state, pr_waiting) VALUES ('t3','c',1,'idle','{')",
        // A request id without a kind, and a kind without an id.
        "INSERT INTO observed_threads (source, ref, linked, state, request_id) VALUES ('t3','c',1,'needs_me','r')",
        "INSERT INTO observed_threads (source, ref, linked, state, request_kind) VALUES ('t3','c',1,'idle','question')",
        // needs_me exactly when there's a request.
        "INSERT INTO observed_threads (source, ref, linked, state) VALUES ('t3','c',1,'needs_me')",
        "INSERT INTO observed_threads (source, ref, linked, state, request_id, request_kind)
         VALUES ('t3','c',1,'running','r','approval')",
    ] {
        refused(&c, bad);
    }
}

#[test]
fn observed_prs_checks() {
    let dir = TempDir::new();
    let c = conn(&dir);
    ok(
        &c,
        "INSERT INTO observed_prs (ref, linked, state, review, checks, ready_since, checked_at)
         VALUES ('a/b#1',1,'open','approved','passing','2026-11-26T10:00:00.000Z','2026-11-26T10:00:00.000Z')",
    );
    ok(
        &c,
        "INSERT INTO observed_prs (ref, linked, error, error_detail, checked_at)
         VALUES ('a/b#2',1,'failed','boom','2026-11-26T10:00:00.000Z')",
    );
    let at = "'2026-11-26T10:00:00.000Z'";
    for bad in [
        format!(
            "INSERT INTO observed_prs (ref, linked, state, checked_at) VALUES ('',1,'open',{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, state, checked_at) VALUES ('a/b#3',3,'open',{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, state, checked_at) VALUES ('a/b#3',1,'draft',{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, state, draft, checked_at) VALUES ('a/b#3',1,'open',2,{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, state, review, checks, checked_at) VALUES ('a/b#3',1,'open','maybe','none',{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, state, checks, checked_at) VALUES ('a/b#3',1,'open','red',{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, error, checked_at) VALUES ('a/b#3',1,'gone',{at})"
        ),
        // A state and an error at once, or neither.
        format!(
            "INSERT INTO observed_prs (ref, linked, state, error, checked_at) VALUES ('a/b#3',1,'open','failed',{at})"
        ),
        format!("INSERT INTO observed_prs (ref, linked, checked_at) VALUES ('a/b#3',1,{at})"),
        // ready_since only while open and not a draft.
        format!(
            "INSERT INTO observed_prs (ref, linked, state, ready_since, checked_at) VALUES ('a/b#3',1,'merged',{at},{at})"
        ),
        format!(
            "INSERT INTO observed_prs (ref, linked, state, draft, ready_since, checked_at) VALUES ('a/b#3',1,'open',1,{at},{at})"
        ),
    ] {
        refused(&c, &bad);
    }
    let missing_checked_at = c
        .execute_batch("INSERT INTO observed_prs (ref, linked, state) VALUES ('a/b#4',1,'open')")
        .unwrap_err();
    assert!(missing_checked_at.to_string().contains("NOT NULL"));
}

#[test]
fn sync_health_checks() {
    let dir = TempDir::new();
    let c = conn(&dir);
    ok(
        &c,
        "INSERT INTO sync_health (source, scope, state, since) VALUES ('gh','me','ok','x');
         INSERT INTO sync_health (source, state, since) VALUES ('t3','broken','x');
         INSERT INTO sync_health (source, scope, state, since, next_try_at) VALUES ('gh','work','rate_limited','x','y');",
    );
    for bad in [
        "INSERT INTO sync_health (source, state, since) VALUES ('slack','ok','x')",
        "INSERT INTO sync_health (source, state, since) VALUES ('claude','sad','x')",
        "INSERT INTO sync_health (source, state, since, failures) VALUES ('claude','ok','x',-1)",
        "INSERT INTO sync_health (source, scope, state, since) VALUES ('gh','other','rate_limited','x')",
    ] {
        refused(&c, bad);
    }
}

#[test]
fn finish_holds_are_one_per_task() {
    let dir = TempDir::new();
    let c = conn(&dir);
    ok(
        &c,
        "INSERT INTO sync_finish_held (task_id, since) VALUES (1,'x')",
    );
    assert!(
        c.execute_batch("INSERT INTO sync_finish_held (task_id, since) VALUES (1,'y')")
            .is_err()
    );
}
