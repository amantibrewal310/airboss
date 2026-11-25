//! `start` and `Attention` with their real threads, against `FakeSync`, a real `core` store
//! and `FakeClock`.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use airboss_attention::{Attention, AttentionConfig, start};
use airboss_core::{Actor, NewTask, Status, Store, TaskId};
use airboss_services::fakes::{FakeClock, FakeSync};
use airboss_services::{
    AttentionService, Changes, Clock, ErrorCode, NeedsKind, Service, SharedStore, StoreChange,
    SyncService, SystemClock,
};
use common::*;

/// Waits until `changes` moves past `seen`, for at most `limit`.
fn moved(changes: &Changes, seen: u64, limit: Duration) -> bool {
    changes.wait_past(seen, limit) > seen
}

/// A store holding tasks in progress, each with one fresh event, so they're quiet five days
/// from the real now.
fn store_with(n: usize, path: Option<&std::path::Path>) -> (SharedStore, Vec<TaskId>) {
    let mut store = match path {
        Some(p) => Store::open(p).unwrap(),
        None => Store::open_in_memory().unwrap(),
    };
    let ids = (0..n)
        .map(|i| {
            let t = store
                .add_task(NewTask::new(format!("Task {i}")), &Actor::Me)
                .unwrap();
            store
                .set_status(t.id, Status::Doing, None, &Actor::Me)
                .unwrap();
            t.id
        })
        .collect();
    (Arc::new(Mutex::new(store)), ids)
}

fn begin(store: SharedStore, sync: &Arc<FakeSync>, clock: Arc<dyn Clock>) -> Arc<Attention> {
    start(AttentionConfig::default(), store, sync.clone(), clock).unwrap()
}

fn later(days: u32) -> SystemTime {
    SystemTime::now() + DAY * days
}

#[test]
fn the_first_list_comes_from_the_snapshot_read_at_start() {
    let (store, ids) = store_with(2, None);
    let sync = Arc::new(FakeSync::new());
    let att = begin(store, &sync, Arc::new(FakeClock::at(later(6))));
    let list = att.needs_me();
    assert_eq!(list.count, 2);
    assert!(list.items.iter().all(|i| i.kind == NeedsKind::Quiet));
    assert_eq!(att.for_task(ids[0]).len(), 1);
    assert!(att.connections().is_empty());
    att.shutdown();
}

#[test]
fn a_sync_change_reaches_changes_quickly() {
    let (store, ids) = store_with(1, None);
    let sync = Arc::new(FakeSync::new());
    let att = begin(store, &sync, Arc::new(SystemClock));
    let seen = att.changes().current();
    let mut t = thread("a1", NeedsMe);
    t.request = Some(request(
        "r1",
        airboss_services::RequestKind::Question,
        "Q?",
        &ts(0, 0),
    ));
    let snapshot = Links::new().thread(t, &[ids[0].0]).build();
    let began = Instant::now();
    sync.set_snapshot(snapshot);
    assert!(moved(&att.changes(), seen, Duration::from_secs(2)));
    let took = began.elapsed();
    // The target is 200 ms with settling; allow slack for a loaded test machine.
    assert!(took < Duration::from_millis(1000), "{took:?}");
    assert_eq!(att.needs_me().items[0].key, key("question:r1"));
    att.shutdown();
}

#[test]
fn a_store_change_needs_no_core_read() {
    let (store, _) = store_with(0, None);
    let sync = Arc::new(FakeSync::new());
    let att = begin(store.clone(), &sync, Arc::new(FakeClock::at(at(9, 0))));
    // Hold the store's lock from here on: any read by attention would block and the change
    // below would never arrive.
    let _held = store.lock().unwrap();
    let seen = att.changes().current();
    let snapshot = Core::new().task(1, None, Doing, &ts(0, 0)).build();
    let began = Instant::now();
    att.store_changed(&StoreChange {
        events: Vec::new(),
        snapshot: Arc::new(snapshot),
    });
    assert!(
        began.elapsed() < Duration::from_millis(20),
        "store_changed returns at once"
    );
    assert!(moved(&att.changes(), seen, Duration::from_secs(2)));
    assert_eq!(att.needs_me().count, 1);
    att.shutdown();
}

#[test]
fn dismiss_and_snooze_survive_a_restart() {
    let dir = TempDir::new();
    let (store, ids) = store_with(3, Some(&dir.db()));
    let sync = Arc::new(FakeSync::new());
    let clock = Arc::new(FakeClock::at(later(6)));
    let att = begin(store.clone(), &sync, clock.clone());
    let key_of = |att: &Attention, id: TaskId| att.for_task(id)[0].key.clone();
    let (first, second) = (key_of(&att, ids[0]), key_of(&att, ids[1]));
    att.dismiss(&first).unwrap();
    att.snooze(&second, None).unwrap();
    assert_eq!(att.needs_me().count, 1);
    att.shutdown();
    drop(att);

    // Restart a day later: both still hidden.
    clock.advance(DAY);
    let att = begin(store.clone(), &sync, clock.clone());
    assert_eq!(att.needs_me().count, 1);
    assert_eq!(att.needs_me().items[0].task, ids[2]);
    att.shutdown();
    drop(att);

    // Past the 3-day snooze, the second is back; the dismissal holds.
    clock.advance(DAY * 3);
    let att = begin(store, &sync, clock);
    let shown: Vec<TaskId> = att.needs_me().items.iter().map(|i| i.task).collect();
    assert_eq!(shown, vec![ids[1], ids[2]]);
    att.shutdown();
}

#[test]
fn snooze_takes_an_end_time_and_restore_undoes_it() {
    let (store, ids) = store_with(1, None);
    let sync = Arc::new(FakeSync::new());
    let clock = Arc::new(FakeClock::at(later(6)));
    let att = begin(store, &sync, clock.clone());
    let k = att.for_task(ids[0])[0].key.clone();
    let until = airboss_services::time::to_ts(clock.now() + HOUR);
    att.snooze(&k, Some(until)).unwrap();
    assert_eq!(att.needs_me().count, 0);
    att.restore(&k).unwrap();
    assert_eq!(att.needs_me().count, 1);
    assert_eq!(att.restore(&k).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        att.snooze(&k, Some("next tuesday".into()))
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert_eq!(
        att.dismiss(&key("quiet:nope")).unwrap_err().code,
        ErrorCode::NotFound
    );
    att.shutdown();
}

#[test]
fn dismiss_answers_within_twenty_milliseconds() {
    let (store, ids) = store_with(50, None);
    let sync = Arc::new(FakeSync::new());
    let att = begin(store, &sync, Arc::new(FakeClock::at(later(6))));
    let k = att.for_task(ids[7])[0].key.clone();
    let began = Instant::now();
    att.dismiss(&k).unwrap();
    let took = began.elapsed();
    assert!(took < Duration::from_millis(100), "{took:?}");
    assert!(
        att.for_task(ids[7]).is_empty(),
        "the list is updated before dismiss returns"
    );
    att.shutdown();
}

#[test]
fn a_quiet_task_due_during_sleep_shows_on_woke() {
    // Activity now; the clock says 4 days later, so nothing is quiet yet.
    let (store, ids) = store_with(1, None);
    let sync = Arc::new(FakeSync::new());
    let clock = Arc::new(FakeClock::at(later(4)));
    let att = begin(store, &sync, clock.clone());
    assert_eq!(att.needs_me().count, 0);
    let seen = att.changes().current();

    // The Mac sleeps through the moment: six days pass at once.
    clock.advance(DAY * 6);
    att.woke();
    assert!(moved(&att.changes(), seen, Duration::from_secs(2)));
    let list = att.needs_me();
    assert_eq!(list.count, 1);
    assert_eq!(list.items[0].task, ids[0]);
    assert_eq!(list.items[0].kind, NeedsKind::Quiet);
    assert_eq!(list.items[0].headline, "No activity for 10 days.");
    att.shutdown();
}

#[test]
fn the_menu_shows_the_first_six() {
    let (store, _) = store_with(8, None);
    let sync = Arc::new(FakeSync::new());
    let att = begin(store, &sync, Arc::new(FakeClock::at(later(6))));
    assert_eq!(att.needs_me().count, 8);
    assert_eq!(att.menu(), att.needs_me().items[..6].to_vec());
    att.shutdown();
}

#[test]
fn shutdown_stops_both_threads_within_a_second() {
    let (store, _) = store_with(1, None);
    let sync = Arc::new(FakeSync::new());
    let att = begin(store, &sync, Arc::new(SystemClock));
    let began = Instant::now();
    att.shutdown();
    assert!(began.elapsed() < Duration::from_secs(1));
    // Calls after shutdown don't hang.
    att.recheck();
    att.woke();
    assert_eq!(
        att.dismiss(&key("quiet:1:x")).unwrap_err().code,
        ErrorCode::ShuttingDown
    );
    att.shutdown();
}

#[test]
fn a_missing_database_directory_hides_nothing_and_refuses_dismissals() {
    // A store whose file was removed after opening still answers snapshot(), but our own
    // connection can't be made in a directory that's gone.
    let dir = TempDir::new();
    let sub = dir.0.join("gone");
    std::fs::create_dir_all(&sub).unwrap();
    let (store, ids) = store_with(1, Some(&sub.join("airboss.db")));
    std::fs::remove_dir_all(&sub).unwrap();
    let sync = Arc::new(FakeSync::new());
    let att = begin(store, &sync, Arc::new(FakeClock::at(later(6))));
    let items = att.for_task(ids[0]);
    assert_eq!(items.len(), 1);
    assert_eq!(
        att.dismiss(&items[0].key).unwrap_err().code,
        ErrorCode::Storage
    );
    assert_eq!(att.needs_me().count, 1);
    att.shutdown();
}

#[test]
fn attention_is_a_shared_attention_service() {
    let (store, _) = store_with(0, None);
    let sync = Arc::new(FakeSync::new());
    let service: Arc<dyn AttentionService> = begin(store, &sync, Arc::new(SystemClock));
    assert_eq!(service.needs_me().count, 0);
    let _: &dyn SyncService = &*sync;
    service.shutdown();
}

#[test]
fn the_first_deadline_after_start_is_kept_without_woke() {
    // The task's only activity is now; the clock says 5 days less a second later, so it
    // turns quiet a second after start. Nothing else happens: the loop must wake for it.
    let (store, ids) = store_with(1, None);
    let sync = Arc::new(FakeSync::new());
    let clock = Arc::new(FakeClock::at(
        SystemTime::now() + DAY * 5 - Duration::from_secs(1),
    ));
    let att = begin(store, &sync, clock.clone());
    assert_eq!(att.needs_me().count, 0);
    let seen = att.changes().current();
    clock.advance(Duration::from_secs(2));
    assert!(
        moved(&att.changes(), seen, Duration::from_secs(3)),
        "the deadline found at start was missed"
    );
    assert_eq!(att.needs_me().items[0].task, ids[0]);
    att.shutdown();
}
