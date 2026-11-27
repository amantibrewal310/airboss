//! Due: an open task whose due date is today or past, in my own time zone. Every case fixes
//! the zone, so the tests read the same anywhere.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, SystemTime};

use airboss_attention::{AttentionConfig, Computed, Hidden, compute};
use airboss_core::{CoreSnapshot, Status};
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{NeedsGroup, NeedsKind, Target};
use common::*;

const IST: i32 = 5 * 3600 + 30 * 60;
const MS: Duration = Duration::from_millis(1);

fn cfg(offset: i32) -> AttentionConfig {
    AttentionConfig {
        utc_offset: Some(offset),
        ..AttentionConfig::default()
    }
}

fn utc(text: &str) -> SystemTime {
    from_ts(text).unwrap()
}

/// Task 7, "Send Vivek the deck", in `status`, due on `date`.
fn due_on(status: Status, date: &str) -> CoreSnapshot {
    Core::new()
        .titled(7, None, status, &ts(0, 0), "Send Vivek the deck")
        .due(date)
        .build()
}

fn at_offset(core: CoreSnapshot, hidden: &[Hidden], now: SystemTime, offset: i32) -> Computed {
    compute(
        &inputs(core, Links::new().build()),
        hidden,
        now,
        &cfg(offset),
    )
}

fn due_items(out: &Computed) -> Vec<&airboss_services::NeedsItem> {
    out.items
        .iter()
        .filter(|i| i.kind == NeedsKind::Due)
        .collect()
}

#[test]
fn the_default_reads_dates_in_the_system_zone() {
    assert_eq!(AttentionConfig::default().utc_offset, None);
    assert_eq!(cfg(IST).utc_offset_at(t0()), IST);
}

#[test]
fn a_task_shows_from_the_first_moment_of_its_day() {
    let core = || due_on(Status::Todo, "2026-11-03");
    let before = at_offset(core(), &[], utc("2026-11-03T00:00:00.000Z") - MS, 0);
    assert!(due_items(&before).is_empty());
    assert_eq!(
        before.next_wake.map(to_ts).as_deref(),
        Some("2026-11-03T00:00:00.000Z"),
        "wakes when the day starts"
    );

    let out = at_offset(core(), &[], utc("2026-11-03T00:00:00.000Z"), 0);
    assert_eq!(out.items.len(), 1);
    let item = &out.items[0];
    assert_eq!(item.key, key("due:7:2026-11-03"));
    assert_eq!(item.kind, NeedsKind::Due);
    assert_eq!(item.group, NeedsGroup::Due);
    assert_eq!(item.task, id(7));
    assert_eq!(item.task_title, "Send Vivek the deck");
    assert_eq!(item.since, "2026-11-03T00:00:00.000Z");
    assert_eq!(item.headline, "Due today");
    assert_eq!(item.context, "Due Tue 3 Nov");
    assert_eq!(item.target, Target::Task(id(7)));
    assert!(item.proposed.is_empty());
    assert!(!item.stale);
    assert_eq!(
        out.next_wake.map(to_ts).as_deref(),
        Some("2026-11-04T00:00:00.000Z"),
        "the headline changes at the next midnight"
    );
}

#[test]
fn the_day_is_my_own_not_utcs() {
    // 3 Nov starts at 18:30 UTC on 2 Nov in India.
    let core = || due_on(Status::Doing, "2026-11-03");
    let starts = utc("2026-11-02T18:30:00.000Z");
    let before = at_offset(core(), &[], starts - MS, IST);
    assert!(due_items(&before).is_empty());
    assert_eq!(before.next_wake, Some(starts));
    let out = at_offset(core(), &[], starts, IST);
    assert_eq!(due_items(&out).len(), 1);
    assert_eq!(out.items[0].since, to_ts(starts));
    // The same moment is still 2 Nov in UTC.
    assert!(due_items(&at_offset(core(), &[], starts, 0)).is_empty());
}

#[test]
fn the_headline_counts_the_days_late() {
    let core = || due_on(Status::Todo, "2026-11-03");
    let headline = |now: &str| {
        let out = at_offset(core(), &[], utc(now), 0);
        (out.items[0].headline.clone(), out.items[0].key.0.clone())
    };
    assert_eq!(headline("2026-11-03T23:59:59.999Z").0, "Due today");
    assert_eq!(headline("2026-11-04T00:00:00.000Z").0, "Due yesterday");
    assert_eq!(headline("2026-11-05T09:00:00.000Z").0, "Overdue by 2 days");
    assert_eq!(headline("2026-11-30T09:00:00.000Z").0, "Overdue by 27 days");
    // One occurrence however late it gets, so a dismissal holds.
    assert_eq!(headline("2026-11-30T09:00:00.000Z").1, "due:7:2026-11-03");
}

#[test]
fn another_years_date_names_the_year() {
    let out = at_offset(due_on(Status::Todo, "2025-12-31"), &[], at(0, 0), 0);
    assert_eq!(out.items[0].context, "Due Wed 31 Dec 2025");
    assert_eq!(out.items[0].headline, "Overdue by 305 days");
}

#[test]
fn only_open_tasks_with_a_date_show() {
    let now = utc("2026-11-10T12:00:00.000Z");
    for (status, shows) in [
        (Status::Todo, true),
        (Status::Doing, true),
        (Status::InReview, true),
        (Status::Blocked, true),
        (Status::Done, false),
        (Status::Dropped, false),
        (Status::Proposed, false),
    ] {
        let out = at_offset(due_on(status, "2026-11-03"), &[], now, 0);
        assert_eq!(due_items(&out).len(), usize::from(shows), "{status:?}");
    }
    let undated = Core::new().task(7, None, Status::Todo, &ts(0, 0)).build();
    assert!(at_offset(undated, &[], now, 0).items.is_empty());
}

#[test]
fn a_finished_task_due_later_sets_no_wake() {
    let out = at_offset(due_on(Status::Done, "2026-11-20"), &[], at(1, 0), 0);
    assert_eq!(out.next_wake, None);
}

#[test]
fn a_dismissal_holds_until_the_date_changes() {
    let now = utc("2026-11-05T09:00:00.000Z");
    let hidden = [Hidden {
        key: key("due:7:2026-11-03"),
        until: None,
    }];
    let out = at_offset(due_on(Status::Todo, "2026-11-03"), &hidden, now, 0);
    assert!(out.items.is_empty());
    assert_eq!(out.live_keys, vec![key("due:7:2026-11-03")]);
    // Moved to another day that has come: a new occurrence.
    let out = at_offset(due_on(Status::Todo, "2026-11-04"), &hidden, now, 0);
    assert_eq!(keys(&out), vec!["due:7:2026-11-04"]);
}

#[test]
fn a_snooze_brings_it_back_at_its_end() {
    let now = utc("2026-11-05T09:00:00.000Z");
    let end = now + DAY;
    let hidden = [Hidden {
        key: key("due:7:2026-11-03"),
        until: Some(end),
    }];
    let out = at_offset(due_on(Status::Todo, "2026-11-03"), &hidden, now, 0);
    assert!(out.items.is_empty());
    assert_eq!(
        out.next_wake,
        Some(end),
        "a hidden item's midnight doesn't wake anything"
    );
    let out = at_offset(due_on(Status::Todo, "2026-11-03"), &hidden, end, 0);
    assert_eq!(keys(&out), vec!["due:7:2026-11-03"]);
}

#[test]
fn due_items_sit_after_stopped_and_before_proposals() {
    let now = utc("2026-11-05T09:00:00.000Z");
    let core = Core::new()
        .titled(1, None, Status::Todo, &ts(0, 0), "Parent")
        .task(2, Some(1), Status::Proposed, &ts(4, 0))
        .titled(3, None, Status::Todo, &ts(0, 0), "Due one")
        .due("2026-11-04")
        .titled(4, None, Status::Todo, &ts(0, 0), "Due other")
        .due("2026-11-05")
        .build();
    let out = at_offset(core, &[], now, 0);
    let groups: Vec<NeedsGroup> = out.items.iter().map(|i| i.group).collect();
    assert_eq!(
        groups,
        vec![NeedsGroup::Due, NeedsGroup::Due, NeedsGroup::Proposed]
    );
    // Newest day first, as in every group but Quiet.
    assert_eq!(out.items[0].task, id(4));
    assert_eq!(out.items[1].task, id(3));
}
