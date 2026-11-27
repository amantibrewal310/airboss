//! The contract's logic: `chip`, `group`, `PrRef`, `time` and the `SyncSnapshot` helpers.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_core::{Actor, NewLink, NewTask, PrState, Store, TaskId};
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{
    Checks, Chip, Clock, ErrorCode, NeedsGroup, NeedsKind, PrRef, PrView, Review, ServiceError,
    Source, SyncSnapshot, SystemClock, ThreadKey, ThreadState, ThreadView,
};

// ---- ThreadState::chip and NeedsKind::group ----

#[test]
fn every_thread_state_has_its_chip() {
    use ThreadState::*;
    let got: Vec<(ThreadState, Option<Chip>)> = [Running, NeedsMe, Errored, Idle, Settled, Gone]
        .into_iter()
        .map(|s| (s, s.chip()))
        .collect();
    assert_eq!(
        got,
        vec![
            (Running, Some(Chip::Working)),
            (NeedsMe, Some(Chip::WaitingOnYou)),
            (Errored, Some(Chip::Stopped)),
            (Idle, Some(Chip::YourTurn)),
            (Settled, Some(Chip::Stopped)),
            (Gone, None),
        ]
    );
}

#[test]
fn every_kind_has_its_group_and_groups_sort_in_display_order() {
    use NeedsKind::*;
    let got: Vec<NeedsGroup> = [
        Question,
        Approval,
        Review,
        FailedRun,
        AgentStopped,
        Proposed,
        Ready,
        Quiet,
    ]
    .into_iter()
    .map(NeedsKind::group)
    .collect();
    assert_eq!(
        got,
        vec![
            NeedsGroup::Questions,
            NeedsGroup::Questions,
            NeedsGroup::Reviews,
            NeedsGroup::FailedRuns,
            NeedsGroup::Stopped,
            NeedsGroup::Proposed,
            NeedsGroup::Ready,
            NeedsGroup::Quiet,
        ]
    );
    let mut sorted = got.clone();
    sorted.sort();
    assert_eq!(sorted, got, "declared in the design's order");
}

// ---- PrRef ----

const PR_INPUTS: &[&str] = &[
    "Acme/Web#42",
    "acme/web#42",
    "  acme/web#42  ",
    "acme/web#012",
    "my.org/my_repo-2#7",
    "https://github.com/Acme/Web/pull/12",
    "https://github.com/acme/web/pull/12/",
    "https://github.com/acme/web/pull/12/files",
    "https://github.com/acme/web/pull/12/commits/4f2a9c1?w=1",
    "https://github.com/acme/web/pull/12#discussion_r123",
    "HTTPS://GitHub.com/ACME/web/PULL/12?tab=checks#top",
    // Refused by both.
    "",
    "acme/web",
    "acme#12",
    "acme/web#",
    "acme/web#0",
    "acme/web#-3",
    "acme/web#12a",
    "acme/web#99999999999999999999999",
    "acme web/site#1",
    "acme/we@b#1",
    "acme/web/extra#1",
    "/web#1",
    "acme/#1",
    "../web#1",
    "acme/..#1",
    "https://github.com/acme/web/issues/12",
    "https://github.com/acme/web/pull/",
    "https://github.com/acme/web",
    "https://gitlab.com/acme/web/pull/12",
    "http://github.com/acme/web/pull/12",
    "https://github.com.evil.example/acme/web/pull/12",
    "github.com/acme/web/pull/12",
    "ünï/côdé#1",
];

#[test]
fn pr_ref_parse_agrees_with_what_core_stores_for_every_input() {
    let mut store = Store::open_in_memory().unwrap();
    for input in PR_INPUTS {
        let task = store.add_task(NewTask::new("t"), &Actor::Me).unwrap().id;
        let stored = store
            .link(task, NewLink::pr(*input), &Actor::Me)
            .ok()
            .map(|l| l.r#ref);
        let parsed = PrRef::parse(input).map(|p| p.as_str().to_string());
        assert_eq!(parsed, stored, "{input:?}");
    }
}

#[test]
fn pr_ref_new_takes_githubs_name_with_owner_and_a_number() {
    assert_eq!(PrRef::new("Acme/Web", 12).unwrap().as_str(), "acme/web#12");
    assert_eq!(PrRef::new("Acme/Web", 0), None);
    assert_eq!(PrRef::new("acme", 3), None);
    assert_eq!(PrRef::new("acme/web#3", 4), None);
}

#[test]
fn pr_ref_parts_give_owner_repo_and_number() {
    let pr = PrRef::parse("https://github.com/My.Org/Web_App/pull/1234").unwrap();
    assert_eq!(pr.parts(), ("my.org", "web_app", 1234));
    assert_eq!(pr.to_string(), "my.org/web_app#1234");
}

// ---- time ----

#[test]
fn to_ts_writes_cores_format_with_milliseconds() {
    let t = UNIX_EPOCH + Duration::from_millis(1_764_091_147_841);
    assert_eq!(to_ts(t), "2025-11-25T17:19:07.841Z");
    assert_eq!(to_ts(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    // Truncated, never rounded up into the next millisecond.
    assert_eq!(
        to_ts(UNIX_EPOCH + Duration::from_nanos(999_999)),
        "1970-01-01T00:00:00.000Z"
    );
    assert_eq!(
        to_ts(UNIX_EPOCH - Duration::from_millis(1)),
        "1969-12-31T23:59:59.999Z"
    );
}

#[test]
fn from_ts_reads_cores_and_githubs_forms() {
    let t = UNIX_EPOCH + Duration::from_millis(1_764_091_147_841);
    assert_eq!(from_ts("2025-11-25T17:19:07.841Z"), Some(t));
    assert_eq!(
        from_ts("2025-11-25T17:19:08Z"),
        Some(UNIX_EPOCH + Duration::from_secs(1_764_091_148))
    );
    assert_eq!(
        from_ts("2025-11-25T17:19:07.5Z"),
        Some(UNIX_EPOCH + Duration::from_millis(1_764_091_147_500))
    );
    assert_eq!(
        from_ts("2024-02-29T00:00:00.123456789Z"),
        Some(UNIX_EPOCH + Duration::new(1_709_164_800, 123_456_789))
    );
}

#[test]
fn from_ts_refuses_anything_else() {
    for bad in [
        "",
        "2025-11-25",
        "2025-11-25 17:19:07Z",
        "2025-11-25T17:19:07",
        "2025-11-25T17:19:07+00:00",
        "2025-11-25T17:19:07.Z",
        "2025-11-25T17:19:07.1234567890Z",
        "2025-13-01T00:00:00Z",
        "2025-02-29T00:00:00Z",
        "2025-04-31T00:00:00Z",
        "2025-11-25T24:00:00Z",
        "2025-11-25T23:60:00Z",
        "2025-11-25T23:59:60Z",
        "2025-1a-25T17:19:07Z",
        "+025-11-25T17:19:07Z",
        "2025-11-25T17:19:07.8é1Z",
    ] {
        assert_eq!(from_ts(bad), None, "{bad:?}");
    }
}

#[test]
fn every_millisecond_round_trips_across_centuries() {
    let mut ms: i64 = -2_208_988_800_000; // 1900-01-01
    while ms < 7_258_118_400_000 {
        // 2200-01-01
        let t = if ms >= 0 {
            UNIX_EPOCH + Duration::from_millis(ms as u64)
        } else {
            UNIX_EPOCH - Duration::from_millis(ms.unsigned_abs())
        };
        let text = to_ts(t);
        assert_eq!(from_ts(&text), Some(t), "{text}");
        ms += 86_399_987; // just under a day, so every time of day comes up
    }
}

#[test]
fn text_order_is_time_order() {
    let times: Vec<SystemTime> = (0..200)
        .map(|i| UNIX_EPOCH + Duration::from_millis(i * 7_919_993_011))
        .collect();
    let mut texts: Vec<String> = times.iter().map(|t| to_ts(*t)).collect();
    texts.sort();
    let back: Vec<SystemTime> = texts.iter().map(|t| from_ts(t).unwrap()).collect();
    assert_eq!(back, times);
}

#[test]
fn time_reads_the_times_core_writes_and_agrees_with_the_clock() {
    let before = SystemClock.now();
    let mut store = Store::open_in_memory().unwrap();
    let t = store.add_task(NewTask::new("t"), &Actor::Me).unwrap();
    let after = SystemClock.now();
    let at = store.history(t.id).unwrap()[0].at.clone();
    let read = from_ts(&at).unwrap();
    assert_eq!(to_ts(read), at, "core's text round-trips");
    let slack = Duration::from_millis(1);
    assert!(read + slack >= before && read <= after, "{at}");
}

// ---- SyncSnapshot helpers ----

fn thread(r: &str, linked: bool, at: Option<&str>) -> ThreadView {
    ThreadView {
        key: ThreadKey {
            source: Source::T3,
            r#ref: r.into(),
        },
        linked,
        state: ThreadState::Idle,
        title: None,
        request: None,
        error: None,
        run_id: None,
        project_path: None,
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: at.map(str::to_string),
        last_reply: None,
        stale: false,
    }
}

fn pr(r: &str, linked: bool, state: PrState, head: &str, updated: &str) -> PrView {
    PrView {
        r#ref: PrRef::parse(r).unwrap(),
        linked,
        url: None,
        title: None,
        state: Some(state),
        draft: false,
        review: Review::None,
        checks: Checks::None,
        additions: None,
        deletions: None,
        head_ref: Some(head.into()),
        author: Some("me".into()),
        updated_at: Some(updated.into()),
        ready_since: None,
        problem: None,
        stale: false,
    }
}

fn snapshot() -> SyncSnapshot {
    let threads = [
        thread("a", true, Some("2026-11-25T10:00:00.000Z")),
        thread("b", true, Some("2026-11-25T12:00:00Z")),
        thread("c", true, None),
        thread("u1", false, Some("2026-11-20T09:00:00.000Z")),
        thread("u2", false, Some("2026-11-24T09:00:00.000Z")),
        thread("u0", false, Some("2026-11-24T09:00:00.000Z")),
    ];
    let prs = [
        pr(
            "acme/web#10",
            true,
            PrState::Open,
            "x",
            "2026-11-25T10:00:00.000Z",
        ),
        pr(
            "acme/web#9",
            true,
            PrState::Merged,
            "x",
            "2026-11-25T10:00:00.000Z",
        ),
        pr(
            "acme/api#3",
            true,
            PrState::Open,
            "x",
            "2026-11-25T10:00:00.000Z",
        ),
        pr(
            "acme/web#20",
            false,
            PrState::Open,
            "feature",
            "2026-11-25T08:00:00.000Z",
        ),
        pr(
            "acme/web#21",
            false,
            PrState::Open,
            "other",
            "2026-11-25T09:30:00Z",
        ),
        pr(
            "acme/web#22",
            false,
            PrState::Closed,
            "feature",
            "2026-11-25T11:00:00.000Z",
        ),
        pr(
            "acme/web#23",
            false,
            PrState::Open,
            "feature",
            "2026-11-25T09:00:00.000Z",
        ),
    ];
    let key = |r: &str| ThreadKey {
        source: Source::T3,
        r#ref: r.into(),
    };
    let one = TaskId(1);
    SyncSnapshot {
        thread_tasks: HashMap::from([
            (key("a"), vec![one]),
            (key("b"), vec![TaskId(2), one]),
            (key("c"), vec![one]),
        ]),
        pr_tasks: HashMap::from([
            (PrRef::parse("acme/web#10").unwrap(), vec![one]),
            (PrRef::parse("acme/web#9").unwrap(), vec![one]),
            (PrRef::parse("acme/api#3").unwrap(), vec![one, TaskId(2)]),
        ]),
        threads: threads.into_iter().map(|t| (t.key.clone(), t)).collect(),
        prs: prs.into_iter().map(|p| (p.r#ref.clone(), p)).collect(),
        projects: vec![],
        checked_at: None,
    }
}

fn thread_refs(v: Vec<&ThreadView>) -> Vec<&str> {
    v.into_iter().map(|t| t.key.r#ref.as_str()).collect()
}

fn pr_refs(v: Vec<&PrView>) -> Vec<&str> {
    v.into_iter().map(|p| p.r#ref.as_str()).collect()
}

#[test]
fn threads_for_a_task_come_newest_first_with_unknown_times_last() {
    let s = snapshot();
    // b's GitHub-style stamp has no milliseconds and still sorts as a time.
    assert_eq!(thread_refs(s.threads_for(TaskId(1))), ["b", "a", "c"]);
    assert_eq!(thread_refs(s.threads_for(TaskId(2))), ["b"]);
    assert!(s.threads_for(TaskId(9)).is_empty());
}

#[test]
fn prs_for_a_task_come_by_owner_repo_and_number() {
    let s = snapshot();
    assert_eq!(
        pr_refs(s.prs_for(TaskId(1))),
        ["acme/api#3", "acme/web#9", "acme/web#10"]
    );
    assert_eq!(pr_refs(s.prs_for(TaskId(2))), ["acme/api#3"]);
}

#[test]
fn untracked_threads_come_newest_first_with_ties_by_key() {
    let s = snapshot();
    assert_eq!(thread_refs(s.untracked()), ["u0", "u2", "u1"]);
}

#[test]
fn my_open_prs_are_the_open_unlinked_ones_most_recently_updated_first() {
    let s = snapshot();
    assert_eq!(
        pr_refs(s.my_open_prs()),
        ["acme/web#21", "acme/web#23", "acme/web#20"]
    );
    assert_eq!(
        pr_refs(s.suggested_prs("feature")),
        ["acme/web#23", "acme/web#20"]
    );
    assert!(s.suggested_prs("nope").is_empty());
}

// ---- ServiceError ----

#[test]
fn a_service_error_displays_its_one_sentence() {
    let e = ServiceError::new(ErrorCode::T3NotRunning, "T3 Code isn't open.");
    assert_eq!(e.to_string(), "T3 Code isn't open.");
    let as_error: Box<dyn std::error::Error> = Box::new(e);
    assert_eq!(as_error.to_string(), "T3 Code isn't open.");
}
