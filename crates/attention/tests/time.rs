//! Thresholds at their edges, and `next_wake`: always later than now, at the right moment.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use airboss_attention::Hidden;
use airboss_services::RequestKind;
use common::*;

const MS: Duration = Duration::from_millis(1);

fn doing_since(days: u64) -> airboss_core::CoreSnapshot {
    Core::new().task(1, None, Doing, &ts(days, 0)).build()
}

#[test]
fn exactly_five_days_is_quiet() {
    let before = run(doing_since(1), Links::new().build(), at(6, 0) - MS);
    assert!(before.items.is_empty());
    let on_time = run(doing_since(1), Links::new().build(), at(6, 0));
    assert_eq!(on_time.items.len(), 1);
}

#[test]
fn next_wake_is_when_the_task_turns_quiet() {
    for now in [at(1, 0), at(2, 7), at(6, 0) - MS] {
        let out = run(doing_since(1), Links::new().build(), now);
        assert_eq!(out.next_wake, Some(at(6, 0)), "{now:?}");
        assert!(out.next_wake.unwrap() > now);
    }
}

#[test]
fn next_wake_is_always_later_than_now() {
    let core = Core::new()
        .task(1, None, Doing, &ts(1, 0))
        .task(2, None, Done, &ts(1, 0))
        .task(3, None, Todo, &ts(1, 0))
        .edge(3, 2)
        .ready(3, &ts(1, 0))
        .build();
    let hidden = [Hidden {
        key: key(&format!("ready:3:{}", ts(1, 0))),
        until: Some(at(3, 0)),
    }];
    let mut now = at(0, 0);
    while now < at(12, 0) {
        let out = run_hidden(core.clone(), Links::new().build(), &hidden, now);
        if let Some(w) = out.next_wake {
            assert!(w > now, "{w:?} at {now:?}");
        }
        now += Duration::from_secs(3_600 * 7 + 13);
    }
}

#[test]
fn after_a_long_sleep_the_quiet_task_is_there_at_once() {
    let before = run(doing_since(1), Links::new().build(), at(2, 0));
    assert!(before.items.is_empty());
    assert_eq!(before.next_wake, Some(at(6, 0)));
    // The Mac slept for 6 days, past the deadline: the next compute has it.
    let after = run(doing_since(1), Links::new().build(), at(8, 0));
    assert_eq!(after.items.len(), 1);
    assert_eq!(after.items[0].headline, "No activity for 7 days.");
}

#[test]
fn a_shown_quiet_item_wakes_when_its_day_count_changes() {
    let out = run(doing_since(1), Links::new().build(), at(7, 3));
    assert_eq!(out.items[0].context, "Quiet for 6 days");
    assert_eq!(out.next_wake, Some(at(8, 0)));
}

#[test]
fn a_ready_item_wakes_when_its_window_ends() {
    let core = Core::new()
        .task(1, None, Done, &ts(0, 0))
        .task(2, None, Todo, &ts(0, 0))
        .edge(2, 1)
        .ready(2, &ts(1, 0))
        .build();
    let out = run(core.clone(), Links::new().build(), at(3, 0));
    assert_eq!(out.next_wake, Some(at(8, 0)));
    let after = run(core, Links::new().build(), at(8, 0));
    assert!(after.items.is_empty());
    assert_eq!(after.next_wake, None);
}

#[test]
fn a_snooze_hides_until_it_ends_and_wakes_then() {
    let quiet = key(&format!("quiet:1:{}", ts(1, 0)));
    let hidden = [Hidden {
        key: quiet.clone(),
        until: Some(at(10, 0)),
    }];
    let during = run_hidden(doing_since(1), Links::new().build(), &hidden, at(7, 0));
    assert!(during.items.is_empty());
    assert_eq!(during.next_wake, Some(at(10, 0)));
    let ended = run_hidden(doing_since(1), Links::new().build(), &hidden, at(10, 0));
    assert_eq!(keys(&ended), vec![quiet.0.clone()]);
}

#[test]
fn a_dismissal_never_ends() {
    let hidden = [Hidden {
        key: key(&format!("quiet:1:{}", ts(1, 0))),
        until: None,
    }];
    let out = run_hidden(doing_since(1), Links::new().build(), &hidden, at(300, 0));
    assert!(out.items.is_empty());
    assert_eq!(out.next_wake, None);
}

#[test]
fn a_snooze_on_a_key_that_isnt_live_wakes_nobody() {
    let hidden = [Hidden {
        key: key("question:gone"),
        until: Some(at(4, 0)),
    }];
    let out = run_hidden(empty_core(), Links::new().build(), &hidden, at(3, 0));
    assert_eq!(out.next_wake, None);
}

#[test]
fn nothing_due_means_no_wake() {
    let core = Core::new().task(1, None, Todo, &ts(0, 0)).build();
    let mut t = thread("a", NeedsMe);
    t.request = Some(request("r", RequestKind::Question, "Q?", &ts(1, 0)));
    let out = run(core, Links::new().thread(t, &[1]).build(), at(2, 0));
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.next_wake, None);
}

#[test]
fn an_active_agent_sets_no_quiet_deadline() {
    let core = doing_since(1);
    let sync = Links::new().thread(thread("a", Running), &[1]).build();
    assert_eq!(run(core, sync, at(2, 0)).next_wake, None);
}

#[test]
fn the_earliest_deadline_wins() {
    let core = Core::new()
        .task(1, None, Doing, &ts(3, 0))
        .task(2, None, Doing, &ts(1, 5))
        .task(3, None, Blocked, &ts(2, 0))
        .build();
    assert_eq!(
        run(core, Links::new().build(), at(4, 0)).next_wake,
        Some(at(6, 5))
    );
}
