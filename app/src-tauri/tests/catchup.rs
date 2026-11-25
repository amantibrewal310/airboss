//! Seeing writes from other processes: a second `Store` on the same file stands in for
//! `airboss mcp`, and the kqueue watcher on the WAL brings its writes in.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use airboss_app::UiEvent;
use airboss_app::dto::Patch;
use airboss_app::watcher::{Watch, wal_path};
use airboss_core::{Actor, NewTask, Store, TaskEdit};
use common::{TestApp, temp_dir};

fn file_app(name: &str) -> (TestApp, PathBuf) {
    let dir = temp_dir(name);
    let db = dir.join("airboss.db");
    let app = TestApp::with_store(Store::open(&db).unwrap());
    app.state.start_background();
    (app, db)
}

fn patch_after(app: &TestApp, seq: u64, within: Duration) -> Option<Patch> {
    match app.shell.wait_for(
        within,
        |e| matches!(e, UiEvent::TreePatch(p) if p.seq > seq),
    ) {
        Some(UiEvent::TreePatch(p)) => Some(p),
        _ => None,
    }
}

fn other(db: &Path) -> Store {
    Store::open(db).unwrap()
}

#[test]
fn another_process_write_shows_within_300_ms() {
    let (app, db) = file_app("other-write");
    let mut agent = other(&db);
    let seq = app.state.seq();
    // Take the median of a few, as measure.sh does; every one must be under 300 ms.
    let mut times = Vec::new();
    let mut last = seq;
    for i in 0..5 {
        let start = Instant::now();
        let t = agent
            .add_task(NewTask::new(format!("From an agent {i}")), &Actor::Me)
            .unwrap();
        let p = patch_after(&app, last, Duration::from_millis(300))
            .unwrap_or_else(|| panic!("no patch within 300 ms (write {i})"));
        times.push(start.elapsed());
        assert!(p.upserts.iter().any(|r| r.id == t.id));
        last = p.seq;
    }
    times.sort();
    eprintln!(
        "other-process write to patch: median {:?}, max {:?}",
        times[2], times[4]
    );
    assert!(
        app.attention
            .calls()
            .iter()
            .any(|c| c.starts_with("store_changed"))
    );
}

#[test]
fn a_burst_of_50_writes_gives_at_most_3_patches() {
    let (app, db) = file_app("burst");
    let mut agent = other(&db);
    let seq = app.state.seq();
    for i in 0..50 {
        agent
            .add_task(NewTask::new(format!("burst {i}")), &Actor::Me)
            .unwrap();
    }
    // Wait for the last one to land, then count.
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.state.vm(|vm| vm.order.len()) < 50 {
        assert!(Instant::now() < deadline, "the burst never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    let patches = app.patches().into_iter().filter(|p| p.seq > seq).count();
    assert!((1..=3).contains(&patches), "{patches} patches");
}

#[test]
fn our_own_write_gives_no_second_patch() {
    let (app, _db) = file_app("own-write");
    let t = app.add("Mine", None);
    let first = app.sync_vm().unwrap();
    app.core(|s| s.edit_task(t, TaskEdit::default().title("Renamed"), &Actor::Me))
        .unwrap();
    let own = app.state.catch_up().unwrap().unwrap();
    assert!(own.seq > first.seq);
    assert!(
        patch_after(&app, own.seq, Duration::from_millis(400)).is_none(),
        "the WAL event and subscribe() found nothing new"
    );
}

#[test]
fn a_repo_added_by_another_handle_updates_repos() {
    let (app, db) = file_app("repo-add");
    let seq = app.state.seq();
    // `repo add` writes no event; catch-up compares repos() too.
    other(&db)
        .add_repo("web", Path::new("/src/web"), None, &Actor::Me)
        .unwrap();
    let p = patch_after(&app, seq, Duration::from_millis(300)).expect("no patch");
    assert_eq!(p.repos.unwrap()[0].name, "web");
}

#[test]
fn focus_reads_nothing_while_data_version_is_unchanged_and_catches_up_when_it_moves() {
    // No watcher here: focus and wake are the safety net on their own.
    let dir = temp_dir("focus");
    let db = dir.join("airboss.db");
    let app = TestApp::with_store(Store::open(&db).unwrap());
    let before = app.attention.calls().len();
    assert!(app.state.check_data_version().unwrap().is_none());
    assert_eq!(
        app.attention.calls().len(),
        before,
        "no store_changed, no rebuild"
    );
    let t = other(&db)
        .add_task(NewTask::new("while away"), &Actor::Me)
        .unwrap();
    let p = app
        .state
        .check_data_version()
        .unwrap()
        .expect("data_version moved");
    assert!(p.upserts.iter().any(|r| r.id == t.id));
    assert!(app.state.check_data_version().unwrap().is_none());
}

#[test]
fn the_wal_watch_re_arms_when_the_wal_is_deleted_and_recreated() {
    let dir = temp_dir("rearm");
    let db = dir.join("x.db");
    let wal = wal_path(&db);
    std::fs::write(&db, b"db").unwrap();
    std::fs::write(&wal, b"one").unwrap();
    let (tx, rx) = mpsc::channel();
    let (moved_tx, moved_rx) = mpsc::channel();
    let _watch = Watch::start(
        &db,
        tx,
        move |why| {
            let _ = moved_tx.send(why.to_string());
        },
        || {},
    )
    .unwrap();
    let wait = |label: &str| {
        rx.recv_timeout(Duration::from_millis(500))
            .unwrap_or_else(|_| panic!("no trigger: {label}"));
        while rx.recv_timeout(Duration::from_millis(50)).is_ok() {}
    };
    std::fs::write(&wal, b"two").unwrap();
    wait("first write");
    std::fs::remove_file(&wal).unwrap();
    wait("delete");
    std::fs::write(&wal, b"three").unwrap();
    wait("recreate");
    // The new file is watched: a plain write to it triggers again.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&wal)
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"four"))
        .unwrap();
    wait("write to the recreated WAL");
    assert!(
        moved_rx.try_recv().is_err(),
        "the database itself didn't move"
    );

    // Replacing the database file is noticed.
    std::fs::rename(&db, dir.join("elsewhere.db")).unwrap();
    let why = moved_rx.recv_timeout(Duration::from_millis(500)).unwrap();
    assert_eq!(why, "The database file was moved or deleted.");
}

#[test]
fn a_moved_database_turns_the_footer_orange() {
    let (app, db) = file_app("moved");
    std::fs::rename(&db, db.with_file_name("gone.db")).unwrap();
    let ev = app.shell.wait_for(Duration::from_millis(500), |e| {
        matches!(e, UiEvent::ConnectionsChanged(_))
    });
    let Some(UiEvent::ConnectionsChanged(c)) = ev else {
        panic!("no connections change")
    };
    assert!(
        c.connections
            .iter()
            .any(|r| r.name == "Database" && r.state == "Database moved" && !r.ok)
    );
}

#[test]
fn an_app_thread_restarts_once_after_a_panic_and_stops_after_a_second() {
    let app = TestApp::new();
    let (tx, rx) = mpsc::channel();
    airboss_app::catchup::guarded(&app.state, "test", move |restarted| {
        let _ = tx.send(restarted);
        panic!("boom");
    });
    assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        "restarted once"
    );
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "not a third time"
    );
    let ev = app.shell.wait_for(Duration::from_secs(1), |e| {
        matches!(e, UiEvent::ConnectionsChanged(_))
    });
    let Some(UiEvent::ConnectionsChanged(c)) = ev else {
        panic!("no connections change")
    };
    assert!(
        c.connections
            .iter()
            .any(|r| r.name == "Live updates" && !r.ok)
    );
    assert!(
        app.shell
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::ErrorBackground(_)))
    );
}

#[test]
fn a_wake_tells_every_service_then_checks_data_version() {
    let (app, db) = file_app("wake");
    let woke = |calls: Vec<String>| calls.iter().any(|c| c == "woke");
    app.state.trigger(airboss_app::catchup::Trigger::Wake);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !(woke(app.sync.calls()) && woke(app.launch.calls()) && woke(app.attention.calls())) {
        assert!(Instant::now() < deadline, "not every service was told");
        std::thread::sleep(Duration::from_millis(10));
    }
    // A write made while asleep is in the view model after the wake (the watcher may bring
    // it in first; either way nothing is missed).
    let t = other(&db)
        .add_task(NewTask::new("while asleep"), &Actor::Me)
        .unwrap();
    let seq = app.state.seq();
    app.state.trigger(airboss_app::catchup::Trigger::Wake);
    let p = patch_after(&app, seq, Duration::from_secs(1)).expect("a patch after the wake");
    assert!(p.upserts.iter().any(|r| r.id == t.id));
}
