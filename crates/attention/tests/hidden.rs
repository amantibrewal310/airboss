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

// ---------- Through the worker ----------

use airboss_attention::{AttentionMsg, AttentionWorker};
use std::sync::Arc;

#[test]
fn dismiss_hides_an_item_until_a_new_occurrence() {
    let (core, sync) = asked("r1");
    let (mut w, fake) = worker(core, sync, at(2, 0));
    let bumps = w.changes().current();
    ask(&mut w, at(2, 1), |r| {
        AttentionMsg::Hide(key("question:r1"), None, r)
    })
    .unwrap();
    assert_eq!(w.needs_me().count, 0);
    assert_eq!(w.changes().current(), bumps + 1);
    // Still hidden much later.
    w.handle(AttentionMsg::Tick, at(90, 0));
    assert_eq!(w.needs_me().count, 0);
    // The agent asks again: a new request, a new key.
    fake.set_snapshot(asked("r2").1);
    w.handle(AttentionMsg::Sync, at(90, 1));
    w.handle(AttentionMsg::Tick, at(90, 2));
    assert_eq!(w.needs_me().items[0].key, key("question:r2"));
}

#[test]
fn snooze_hides_until_its_end() {
    let core = Core::new().task(1, None, Doing, &ts(0, 0)).build();
    let (mut w, _) = worker(core, Links::new().build(), at(6, 0));
    let quiet = w.needs_me().items[0].key.clone();
    ask(&mut w, at(6, 0), |r| {
        AttentionMsg::Hide(quiet.clone(), Some(at(9, 0)), r)
    })
    .unwrap();
    assert_eq!(w.needs_me().count, 0);
    w.handle(
        AttentionMsg::Tick,
        at(9, 0) - std::time::Duration::from_millis(1),
    );
    assert_eq!(w.needs_me().count, 0);
    w.handle(AttentionMsg::Tick, at(9, 0));
    assert_eq!(w.needs_me().items[0].key, quiet);
}

#[test]
fn restore_brings_an_item_back() {
    let (core, sync) = asked("r1");
    let (mut w, _) = worker(core, sync, at(2, 0));
    ask(&mut w, at(2, 0), |r| {
        AttentionMsg::Hide(key("question:r1"), None, r)
    })
    .unwrap();
    ask(&mut w, at(2, 0), |r| {
        AttentionMsg::Restore(key("question:r1"), r)
    })
    .unwrap();
    assert_eq!(w.needs_me().count, 1);
    let err = ask(&mut w, at(2, 0), |r| {
        AttentionMsg::Restore(key("question:r1"), r)
    })
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn hiding_a_key_that_isnt_live_is_not_found() {
    let (mut w, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    let err = ask(&mut w, at(2, 0), |r| {
        AttentionMsg::Hide(key("question:nope"), None, r)
    })
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn a_dismiss_that_cant_be_saved_leaves_the_item() {
    let (core, sync) = asked("r1");
    let fake = Arc::new(airboss_services::fakes::FakeSync::new());
    fake.set_snapshot(sync);
    let mut w = AttentionWorker::new(
        Default::default(),
        Arc::new(core),
        fake,
        HiddenStore::unavailable(),
        at(2, 0),
    );
    let err = ask(&mut w, at(2, 0), |r| {
        AttentionMsg::Hide(key("question:r1"), None, r)
    })
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Storage);
    assert_eq!(w.needs_me().count, 1);
}

#[test]
fn dismiss_and_snooze_survive_a_restart() {
    let dir = TempDir::new();
    let core = Core::new()
        .task(1, None, Doing, &ts(0, 0))
        .task(2, None, Doing, &ts(0, 1))
        .build();
    let start = |now| {
        let fake = Arc::new(airboss_services::fakes::FakeSync::new());
        AttentionWorker::new(
            Default::default(),
            Arc::new(core.clone()),
            fake,
            HiddenStore::open(&dir.db()).unwrap(),
            now,
        )
    };
    let mut w = start(at(6, 0));
    let keys: Vec<_> = w.needs_me().items.iter().map(|i| i.key.clone()).collect();
    assert_eq!(keys.len(), 2);
    ask(&mut w, at(6, 0), |r| {
        AttentionMsg::Hide(keys[0].clone(), None, r)
    })
    .unwrap();
    ask(&mut w, at(6, 0), |r| {
        AttentionMsg::Hide(keys[1].clone(), Some(at(9, 0)), r)
    })
    .unwrap();
    drop(w);

    let w = start(at(7, 0));
    assert_eq!(w.needs_me().count, 0);
    let w = start(at(9, 0));
    assert_eq!(
        w.needs_me()
            .items
            .iter()
            .map(|i| i.key.clone())
            .collect::<Vec<_>>(),
        vec![keys[1].clone()]
    );
}

#[test]
fn rows_not_live_for_thirty_days_are_pruned() {
    let dir = TempDir::new();
    let (core, sync) = asked("r1");
    let fake = Arc::new(airboss_services::fakes::FakeSync::new());
    fake.set_snapshot(sync);
    let mut w = AttentionWorker::new(
        Default::default(),
        Arc::new(core),
        fake.clone(),
        HiddenStore::open(&dir.db()).unwrap(),
        at(2, 0),
    );
    let on_disk = HiddenStore::open(&dir.db()).unwrap();
    let k = key("question:r1");
    ask(&mut w, at(2, 0), |r| AttentionMsg::Hide(k.clone(), None, r)).unwrap();
    // The question is answered, so its key stops being live; it was last live at day 2.
    fake.set_snapshot(Links::new().build());
    w.handle(AttentionMsg::Sync, at(3, 0));
    w.handle(AttentionMsg::Tick, at(3, 1));
    // A daily pass a day short of 30 days keeps it.
    w.handle(AttentionMsg::Tick, at(31, 0));
    assert!(on_disk.contains(&k).unwrap());
    // The next daily pass, exactly 30 days after it was last live, deletes it.
    w.handle(AttentionMsg::Tick, at(32, 0));
    assert!(!on_disk.contains(&k).unwrap());
    let err = ask(&mut w, at(33, 1), |r| AttentionMsg::Restore(k.clone(), r)).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn pruning_runs_at_most_once_a_day() {
    let dir = TempDir::new();
    let (core, sync) = asked("r1");
    let fake = Arc::new(airboss_services::fakes::FakeSync::new());
    fake.set_snapshot(sync);
    let mut w = AttentionWorker::new(
        Default::default(),
        Arc::new(core),
        fake.clone(),
        HiddenStore::open(&dir.db()).unwrap(),
        at(2, 0),
    );
    let on_disk = HiddenStore::open(&dir.db()).unwrap();
    let k = key("question:r1");
    ask(&mut w, at(2, 0), |r| AttentionMsg::Hide(k.clone(), None, r)).unwrap();
    fake.set_snapshot(Links::new().build());
    w.handle(AttentionMsg::Sync, at(3, 0));
    // A pass at 31 days 23 hours keeps it (29.96 days since it was live) ...
    w.handle(AttentionMsg::Tick, at(31, 23));
    // ... and the next pass can't run until a day after that one, even though it's due.
    w.handle(AttentionMsg::Tick, at(32, 1));
    assert!(on_disk.contains(&k).unwrap());
    w.handle(AttentionMsg::Tick, at(32, 23));
    assert!(!on_disk.contains(&k).unwrap());
}

#[test]
fn a_live_hidden_row_is_never_pruned() {
    let (core, sync) = asked("r1");
    let (mut w, _) = worker(core, sync, at(2, 0));
    ask(&mut w, at(2, 0), |r| {
        AttentionMsg::Hide(key("question:r1"), None, r)
    })
    .unwrap();
    for day in [20, 40, 60, 80] {
        w.handle(AttentionMsg::Tick, at(day, 0));
    }
    assert_eq!(w.needs_me().count, 0);
    ask(&mut w, at(80, 1), |r| {
        AttentionMsg::Restore(key("question:r1"), r)
    })
    .unwrap();
}

#[test]
fn an_agent_stopped_row_is_saved_and_read_back_with_its_kind() {
    let dir = TempDir::new();
    let store = HiddenStore::open(&dir.db()).unwrap();
    let stopped = HiddenRow {
        key: key("stopped:7:2026-11-02T10:00:00.000Z"),
        kind: NeedsKind::AgentStopped,
        task: TaskId(7),
        until: Some(at(4, 0)),
    };
    store.hide(&stopped).unwrap();
    drop(store);
    assert_eq!(
        HiddenStore::open(&dir.db()).unwrap().load().unwrap(),
        vec![stopped]
    );
}
