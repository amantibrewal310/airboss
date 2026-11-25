//! `LaunchWorker::handle` with chosen times: when it settles, what it checks, and when it
//! next wakes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, SystemTime};

use airboss_launch::fake::{Reply, Version};
use airboss_launch::{CoreWrite, FailWith, LaunchMsg, T3_MISSING};
use airboss_services::{Availability, LaunchService, Service};
use common::{H, do_it, plan_it, t0, t3_key};

const MIN: Duration = Duration::from_secs(60);
const CAP: Duration = Duration::from_secs(15 * 60);

fn secs(n: u64) -> SystemTime {
    t0() + Duration::from_secs(n)
}

fn finds(h: &H) -> usize {
    h.sync
        .calls()
        .iter()
        .filter(|c| c.starts_with("find_thread"))
        .count()
}

#[test]
fn nothing_pending_means_one_wake_every_15_minutes() {
    let mut h = H::new();
    for msg in [LaunchMsg::Tick, LaunchMsg::Settle, LaunchMsg::Woke] {
        let step = h.worker.handle(msg, t0());
        assert!(!step.changed);
        assert_eq!(step.next_wake, t0() + CAP);
    }
}

#[test]
fn a_pending_do_it_row_is_settled_at_10_30_60_and_180_seconds() {
    let mut h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.t3.push(Reply::Timeout);
    h.launch.start(&do_it(t.id)).unwrap();
    let before = finds(&h);
    let mut now = t0();
    let mut wakes = Vec::new();
    for _ in 0..5 {
        let step = h.worker.handle(LaunchMsg::Tick, now);
        assert!(step.next_wake > now);
        wakes.push(step.next_wake);
        now = step.next_wake;
    }
    assert_eq!(
        wakes,
        [secs(10), secs(30), secs(60), secs(180), secs(180) + CAP]
    );
    assert_eq!(h.rows(t.id)[0].2, "lost");
    assert_eq!(finds(&h) - before, 5);
}

#[test]
fn woke_catches_up_on_an_overdue_row() {
    let mut h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.t3.push(Reply::Timeout);
    h.launch.start(&do_it(t.id)).unwrap();
    h.sync.answer_find_thread(
        h.path_of(&web),
        &format!("task-{}-t", t.id.0),
        t3_key("found-on-wake"),
    );
    // The Mac slept through every deadline.
    let step = h.worker.handle(LaunchMsg::Woke, secs(3 * 3600));
    assert!(step.changed);
    assert_eq!(h.rows(t.id)[0].3.as_deref(), Some("found-on-wake"));
    assert_eq!(step.next_wake, secs(3 * 3600) + CAP);
}

#[test]
fn woke_and_recheck_queue_their_work() {
    let mut h = H::new();
    while h.worker.try_next().is_some() {}
    h.launch.woke();
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::Woke));
    h.launch.recheck();
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::CheckAvailability));
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::Settle));
    assert_eq!(h.worker.try_next(), None);
}

#[test]
fn a_failed_link_is_retried_a_minute_later() {
    let mut h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core.fail_next(CoreWrite::Link, FailWith::Db);
    h.launch.start(&do_it(t.id)).unwrap();
    assert!(h.store.lock().unwrap().links(t.id).unwrap().is_empty());
    let seen = h.launch.changes().current();
    // The next pass is due within a minute.
    let step = h.worker.handle(LaunchMsg::Tick, secs(1));
    assert!(step.changed);
    assert_eq!(h.store.lock().unwrap().links(t.id).unwrap().len(), 1);
    assert!(
        h.launch
            .prepare(t.id, airboss_services::Mode::DoIt)
            .unwrap()
            .started[0]
            .linked
    );
    assert!(h.launch.changes().current() > seen);
    assert_eq!(step.next_wake, secs(1) + CAP);
}

#[test]
fn an_unlinked_row_wakes_the_worker_within_a_minute() {
    let mut h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core.fail_next(CoreWrite::Link, FailWith::Db);
    h.launch.start(&do_it(t.id)).unwrap();
    h.core.fail_next(CoreWrite::Link, FailWith::Db);
    let step = h.worker.handle(LaunchMsg::Tick, secs(5));
    assert!(!step.changed);
    assert_eq!(step.next_wake, secs(5) + MIN);
}

#[test]
fn a_deleted_tasks_rows_are_forgotten() {
    let mut h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core.fail_next(CoreWrite::Link, FailWith::NotFound);
    h.launch.start(&do_it(t.id)).unwrap();
    assert!(h.rows(t.id).is_empty());
    let step = h.worker.handle(LaunchMsg::Tick, secs(5));
    assert_eq!(step.next_wake, secs(5) + CAP);
}

#[test]
fn next_wake_is_always_after_now() {
    let mut h = H::new();
    let web = h.repo("web");
    let api = h.repo("api");
    let a = h.task("A", &[&web, &api]);
    let b = h.task("B", &[&web]);
    h.t3.push(Reply::Timeout);
    h.launch.start(&do_it(a.id)).unwrap();
    h.clock.advance(Duration::from_secs(7));
    h.t3.push(Reply::Timeout);
    h.launch.start(&do_it(b.id)).unwrap();
    h.t3.push(Reply::Timeout);
    h.launch.start(&plan_it(b.id)).unwrap();
    let msgs = [
        LaunchMsg::Tick,
        LaunchMsg::Settle,
        LaunchMsg::Woke,
        LaunchMsg::CheckAvailability,
    ];
    for n in [
        0, 1, 9, 10, 11, 17, 30, 37, 59, 60, 61, 179, 180, 187, 200, 5000,
    ] {
        for msg in msgs {
            let now = secs(n);
            let step = h.worker.handle(msg, now);
            assert!(
                step.next_wake > now,
                "{msg:?} at {n}s gave {:?}",
                step.next_wake
            );
            assert!(step.next_wake <= now + CAP);
        }
    }
    // Wall-clock jumps backwards are fine too.
    let step = h
        .worker
        .handle(LaunchMsg::Tick, t0() - Duration::from_secs(3600));
    assert!(step.next_wake > t0() - Duration::from_secs(3600));
}

#[test]
fn availability_ready_missing_and_back() {
    let mut h = H::new();
    assert_eq!(h.launch.availability(), Availability::Unknown);
    assert_eq!(h.launch.connections()[0].state, "Checking…");
    let seen = h.launch.changes().current();
    let step = h.worker.handle(LaunchMsg::CheckAvailability, t0());
    assert!(step.changed);
    assert_eq!(
        h.launch.availability(),
        Availability::Ready {
            version: "0.1.12".into()
        }
    );
    assert!(h.launch.changes().current() > seen);
    let c = &h.launch.connections()[0];
    assert_eq!(
        (c.name.as_str(), c.ok, c.state.as_str()),
        ("T3 Code", true, "Connected")
    );
    assert_eq!(c.detail, "Do it and Plan it start agent threads here.");

    // Unchanged: no bump.
    let seen = h.launch.changes().current();
    assert!(!h.worker.handle(LaunchMsg::CheckAvailability, t0()).changed);
    assert_eq!(h.launch.changes().current(), seen);

    // The launcher says the app is gone.
    h.t3.set_version(Version::Exit(
        127,
        "T3 Code has moved or been removed.".into(),
    ));
    assert!(h.worker.handle(LaunchMsg::CheckAvailability, t0()).changed);
    assert_eq!(
        h.launch.availability(),
        Availability::Missing {
            reason: T3_MISSING.into()
        }
    );
    let c = &h.launch.connections()[0];
    assert_eq!((c.ok, c.state.as_str()), (false, "Not found"));
    assert_eq!(
        c.fix.as_deref(),
        Some("Install T3 Code to turn on Do it and Plan it. Everything else works.")
    );

    // No launcher at all: missing without running anything.
    h.t3.set_version(Version::Prints("t3 v0.2.0\n".into()));
    std::fs::remove_file(&h.cfg.t3_path).unwrap();
    let calls = h.t3.calls().len();
    h.worker.handle(LaunchMsg::CheckAvailability, t0());
    assert!(matches!(
        h.launch.availability(),
        Availability::Missing { .. }
    ));
    assert_eq!(h.t3.calls().len(), calls);

    // Installed again.
    std::fs::write(&h.cfg.t3_path, "").unwrap();
    h.worker.handle(LaunchMsg::CheckAvailability, t0());
    assert_eq!(
        h.launch.availability(),
        Availability::Ready {
            version: "0.2.0".into()
        }
    );
}

#[test]
fn shutdown_needs_no_work() {
    let mut h = H::new();
    let step = h.worker.handle(LaunchMsg::Shutdown, t0());
    assert!(!step.changed);
    assert!(step.next_wake > t0());
}
