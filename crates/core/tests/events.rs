//! Rule 23: every committed change tells subscribers which events it wrote.
#![allow(clippy::unwrap_used)]

use airboss_core::{Actor, EventId, NewLink, NewTask, Store};

fn ids_of(store: &Store, task: airboss_core::TaskId) -> Vec<EventId> {
    store
        .history(task)
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect()
}

#[test]
fn a_call_sends_exactly_the_ids_of_the_events_it_wrote() {
    let mut store = Store::open_in_memory().unwrap();
    let rx = store.subscribe();
    let t = store
        .add_task(NewTask::new("Booking API"), &Actor::Me)
        .unwrap();

    assert_eq!(rx.try_recv().unwrap(), ids_of(&store, t.id));
    assert!(rx.try_recv().is_err(), "one message per call");
}

#[test]
fn every_subscriber_receives_every_change() {
    let mut store = Store::open_in_memory().unwrap();
    let app = store.subscribe();
    let mcp = store.subscribe();
    let t = store.add_task(NewTask::new("One"), &Actor::Me).unwrap();
    store.add_note(t.id, "started", &Actor::Me).unwrap();

    let expected = ids_of(&store, t.id);
    for rx in [&app, &mcp] {
        let got: Vec<EventId> = rx.try_iter().flatten().collect();
        assert_eq!(got, expected);
    }
}

#[test]
fn a_call_that_changes_nothing_sends_nothing() {
    let mut store = Store::open_in_memory().unwrap();
    let t = store.add_task(NewTask::new("One"), &Actor::Me).unwrap();
    store.link(t.id, NewLink::pr("o/r#1"), &Actor::Me).unwrap();
    let rx = store.subscribe();

    // Linking the same PR again returns the existing link and writes no event.
    store.link(t.id, NewLink::pr("o/r#1"), &Actor::Me).unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_refused_call_sends_nothing() {
    let mut store = Store::open_in_memory().unwrap();
    let rx = store.subscribe();
    assert!(store.add_task(NewTask::new("  "), &Actor::Me).is_err());
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_dropped_subscriber_does_not_stop_writes_or_other_subscribers() {
    let mut store = Store::open_in_memory().unwrap();
    let gone = store.subscribe();
    let kept = store.subscribe();
    drop(gone);
    store.add_task(NewTask::new("One"), &Actor::Me).unwrap();
    assert_eq!(kept.try_recv().unwrap().len(), 1);
}

#[test]
fn events_after_a_cursor_come_back_in_order_across_tasks() {
    let mut store = Store::open_in_memory().unwrap();
    let a = store.add_task(NewTask::new("A"), &Actor::Me).unwrap();
    let cursor = ids_of(&store, a.id)[0];
    let b = store.add_task(NewTask::new("B"), &Actor::Me).unwrap();
    store.add_note(a.id, "later", &Actor::Me).unwrap();

    let after = store.events_after(cursor).unwrap();
    let tasks: Vec<_> = after.iter().map(|e| e.task).collect();
    assert_eq!(tasks, vec![b.id, a.id]);
    assert!(after.windows(2).all(|w| w[0].id < w[1].id));
    assert!(store.events_after(after[1].id).unwrap().is_empty());
}

#[test]
fn events_after_zero_returns_everything() {
    let mut store = Store::open_in_memory().unwrap();
    store.add_task(NewTask::new("A"), &Actor::Me).unwrap();
    store.add_task(NewTask::new("B"), &Actor::Me).unwrap();
    assert_eq!(store.events_after(EventId(0)).unwrap().len(), 2);
}

#[test]
fn an_event_id_is_never_handed_out_twice_even_after_its_task_is_deleted() {
    let mut store = Store::open_in_memory().unwrap();
    let keep = store.add_task(NewTask::new("Keep"), &Actor::Me).unwrap();
    let doomed = store.add_task(NewTask::new("Doomed"), &Actor::Me).unwrap();
    for text in ["one", "two", "three"] {
        store.add_note(doomed.id, text, &Actor::Me).unwrap();
    }
    // The newest ids all belong to the doomed task; deleting it writes no event of its own.
    let highest = store.latest_event_id().unwrap();
    assert_eq!(ids_of(&store, doomed.id).last(), Some(&highest));
    store.delete_task(doomed.id, &Actor::Me).unwrap();
    assert!(store.latest_event_id().unwrap() < highest);

    store.add_note(keep.id, "after", &Actor::Me).unwrap();
    let new = *ids_of(&store, keep.id).last().unwrap();
    assert!(
        new > highest,
        "{new:?} was handed out before (up to {highest:?})"
    );
    // So a reader holding `highest` as its cursor still sees the new event.
    let caught_up = store.events_after(highest).unwrap();
    assert_eq!(
        caught_up.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![new]
    );
}
