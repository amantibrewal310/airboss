//! Dismiss and snooze: the table on its own, then through the worker.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_attention::{HiddenRow, HiddenStore};
use airboss_core::TaskId;
use airboss_services::{ErrorCode, NeedsKind};
use common::*;

fn row(k: &str, until: Option<std::time::SystemTime>) -> HiddenRow {
    HiddenRow {
        key: key(k),
        kind: NeedsKind::Quiet,
        task: TaskId(1),
        until,
    }
}

#[test]
fn rows_survive_reopening() {
    let dir = TempDir::new();
    let store = HiddenStore::open(&dir.db()).unwrap();
    store.hide(&row("quiet:1:a", None)).unwrap();
    store.hide(&row("quiet:2:b", Some(at(4, 0)))).unwrap();
    drop(store);
    let mut rows = HiddenStore::open(&dir.db()).unwrap().load().unwrap();
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    assert_eq!(
        rows,
        vec![row("quiet:1:a", None), row("quiet:2:b", Some(at(4, 0)))]
    );
}

#[test]
fn hiding_a_hidden_key_replaces_its_row() {
    let store = HiddenStore::in_memory().unwrap();
    store.hide(&row("quiet:1:a", Some(at(4, 0)))).unwrap();
    store.hide(&row("quiet:1:a", None)).unwrap();
    assert_eq!(store.load().unwrap(), vec![row("quiet:1:a", None)]);
}

#[test]
fn restore_deletes_the_row_and_says_whether_there_was_one() {
    let store = HiddenStore::in_memory().unwrap();
    store.hide(&row("quiet:1:a", None)).unwrap();
    assert!(store.restore(&key("quiet:1:a")).unwrap());
    assert!(!store.restore(&key("quiet:1:a")).unwrap());
    assert!(store.load().unwrap().is_empty());
}

#[test]
fn prune_deletes_exactly_the_keys_given() {
    let store = HiddenStore::in_memory().unwrap();
    for k in ["a", "b", "c"] {
        store.hide(&row(k, None)).unwrap();
    }
    store.prune(&[key("a"), key("c"), key("zzz")]).unwrap();
    assert_eq!(store.load().unwrap(), vec![row("b", None)]);
}

#[test]
fn an_unavailable_store_hides_nothing_and_refuses_writes() {
    let store = HiddenStore::unavailable();
    assert!(store.load().unwrap().is_empty());
    assert_eq!(
        store.hide(&row("a", None)).unwrap_err().code,
        ErrorCode::Storage
    );
    assert_eq!(
        store.restore(&key("a")).unwrap_err().code,
        ErrorCode::Storage
    );
}

#[test]
fn a_directory_that_cant_hold_a_database_is_a_storage_error() {
    let dir = TempDir::new();
    let err = HiddenStore::open(&dir.0.join("missing").join("airboss.db")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Storage);
}
