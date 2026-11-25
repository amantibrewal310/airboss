//! `AttentionWorker::handle` with chosen times: settling, deadlines, version and changes.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use airboss_attention::{AttentionMsg, MAX_WAIT, Step};
use common::*;

const MS: Duration = Duration::from_millis(1);

#[test]
fn the_first_list_is_ready_at_once() {
    let (core, sync) = asked("r1");
    let (w, _) = worker(core, sync, at(2, 0));
    let list = w.needs_me();
    assert_eq!(list.count, 1);
    assert_eq!(list.items[0].key, key("question:r1"));
    assert_eq!(list.version, 1);
    assert_eq!(w.changes().current(), 1);
}

#[test]
fn a_store_message_brings_its_own_snapshot() {
    let (mut w, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    let core = Core::new().task(1, None, Doing, &ts(0, 0)).build();
    w.handle(AttentionMsg::Store(Arc::new(core)), at(9, 0));
    w.handle(AttentionMsg::Tick, at(9, 0) + 150 * MS);
    assert_eq!(w.needs_me().count, 1);
}

#[test]
fn a_burst_of_twenty_changes_makes_one_recompute() {
    let (mut w, sync) = worker(empty_core(), Links::new().build(), at(2, 0));
    let start = w.recomputes();
    let t = at(9, 0);
    for i in 0..20u32 {
        let core = Core::new().task(1, None, Doing, &ts(0, 0)).build();
        let msg = match i % 2 {
            0 => AttentionMsg::Store(Arc::new(core)),
            _ => AttentionMsg::Sync,
        };
        assert_eq!(w.handle(msg, t + MS * i * 5), Step::Wait(t + 150 * MS));
    }
    assert_eq!(w.recomputes(), start);
    w.handle(AttentionMsg::Tick, t + 150 * MS);
    assert_eq!(w.recomputes(), start + 1);
    assert_eq!(w.needs_me().count, 1);
    drop(sync);
}

#[test]
fn a_steady_stream_cant_postpone_the_recompute() {
    let (mut w, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    let start = w.recomputes();
    let t = at(3, 0);
    // One message every 100 ms: the window from the first closes at 150 ms.
    w.handle(AttentionMsg::Sync, t);
    w.handle(AttentionMsg::Sync, t + 100 * MS);
    assert_eq!(w.recomputes(), start);
    w.handle(AttentionMsg::Sync, t + 200 * MS);
    assert_eq!(w.recomputes(), start + 1);
}

#[test]
fn version_and_changes_move_only_on_a_visible_change() {
    let (core, sync) = asked("r1");
    let (mut w, fake) = worker(core.clone(), sync.clone(), at(2, 0));
    let (version, bumps) = (w.needs_me().version, w.changes().current());

    // The same inputs again: nothing visible changed.
    w.handle(AttentionMsg::Store(Arc::new(core.clone())), at(2, 1));
    w.handle(AttentionMsg::Tick, at(2, 2));
    assert_eq!(
        (w.needs_me().version, w.changes().current()),
        (version, bumps)
    );

    // Only checked_at moved: the list carries it, without a new version.
    let mut later = sync.clone();
    later.checked_at = Some(ts(2, 3));
    fake.set_snapshot(later);
    w.handle(AttentionMsg::Sync, at(2, 3));
    w.handle(AttentionMsg::Tick, at(2, 4));
    assert_eq!(w.needs_me().checked_at, Some(ts(2, 3)));
    assert_eq!(
        (w.needs_me().version, w.changes().current()),
        (version, bumps)
    );

    // A new question is a visible change.
    let (_, sync2) = asked("r2");
    fake.set_snapshot(sync2);
    w.handle(AttentionMsg::Sync, at(2, 5));
    w.handle(AttentionMsg::Tick, at(2, 6));
    assert_eq!(w.needs_me().items[0].key, key("question:r2"));
    assert_eq!(w.needs_me().version, version + 1);
    assert_eq!(w.changes().current(), bumps + 1);
}

#[test]
fn woke_after_a_six_day_sleep_shows_quiet_at_once() {
    let core = Core::new().task(1, None, Doing, &ts(1, 0)).build();
    let (mut w, _) = worker(core, Links::new().build(), at(2, 0));
    assert_eq!(w.needs_me().count, 0);
    // Before the sleep the worker would have woken on the deadline, capped at 15 minutes.
    assert_eq!(
        w.handle(AttentionMsg::Tick, at(2, 0)),
        Step::Wait(at(2, 0) + MAX_WAIT)
    );
    // The Mac slept six days; the first thing that arrives is woke().
    w.handle(AttentionMsg::Woke, at(8, 0));
    let list = w.needs_me();
    assert_eq!(list.count, 1);
    assert_eq!(list.items[0].headline, "No activity for 7 days.");
}

#[test]
fn the_wait_is_the_next_deadline_capped_at_fifteen_minutes() {
    let core = Core::new().task(1, None, Doing, &ts(1, 0)).build();
    let (mut w, _) = worker(core, Links::new().build(), at(2, 0));
    let near = at(6, 0) - Duration::from_secs(120);
    assert_eq!(w.handle(AttentionMsg::Tick, near), Step::Wait(at(6, 0)));
    // Now it's quiet; the next change is its day count, more than 15 minutes away.
    assert_eq!(
        w.handle(AttentionMsg::Tick, at(6, 0)),
        Step::Wait(at(6, 0) + MAX_WAIT)
    );
    assert_eq!(w.needs_me().count, 1);

    let (mut idle, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    assert_eq!(
        idle.handle(AttentionMsg::Recheck, at(2, 0)),
        Step::Wait(at(2, 0) + MAX_WAIT)
    );
}

#[test]
fn a_pending_change_wakes_the_loop_when_its_window_closes() {
    let (mut w, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    assert_eq!(
        w.handle(AttentionMsg::Sync, at(2, 0)),
        Step::Wait(at(2, 0) + 150 * MS)
    );
}

#[test]
fn recheck_woke_and_tick_recompute_at_once() {
    let (mut w, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    let start = w.recomputes();
    for msg in [
        AttentionMsg::Recheck,
        AttentionMsg::Woke,
        AttentionMsg::Tick,
    ] {
        w.handle(msg, at(2, 0));
    }
    assert_eq!(w.recomputes(), start + 3);
}

#[test]
fn shutdown_stops() {
    let (mut w, _) = worker(empty_core(), Links::new().build(), at(2, 0));
    assert_eq!(w.handle(AttentionMsg::Shutdown, at(2, 0)), Step::Stop);
}

#[test]
fn for_task_shows_a_shared_thread_on_each_of_its_tasks() {
    let core = Core::new()
        .task(3, None, Doing, &ts(1, 0))
        .task(7, None, Doing, &ts(1, 0))
        .task(9, None, Done, &ts(1, 0))
        .build();
    let mut t = thread("a1", NeedsMe);
    t.request = Some(request(
        "r1",
        airboss_services::RequestKind::Question,
        "Q?",
        &ts(1, 1),
    ));
    let sync = Links::new().thread(t, &[7, 3, 9]).build();
    let (w, _) = worker(core, sync, at(2, 0));
    assert_eq!(w.needs_me().count, 1);
    assert_eq!(w.needs_me().items[0].task, id(3));
    assert_eq!(w.for_task(id(3)).len(), 1);
    assert_eq!(w.for_task(id(7)).len(), 1);
    assert!(
        w.for_task(id(9)).is_empty(),
        "a closed task shows no callout"
    );
}

#[test]
fn for_task_keeps_list_order() {
    let core = Core::new()
        .task(1, None, Doing, &ts(0, 0))
        .task(2, Some(1), Proposed, &ts(0, 0))
        .build();
    let sync = Links::new().thread(thread("a1", Errored), &[1]).build();
    let (w, _) = worker(core, sync, at(9, 0));
    let kinds: Vec<_> = w.for_task(id(1)).iter().map(|i| i.kind).collect();
    use airboss_services::NeedsKind as K;
    assert_eq!(kinds, vec![K::FailedRun, K::Proposed, K::Quiet]);
}
