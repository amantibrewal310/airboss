//! Agent stopped: a `doing` task whose every linked thread has stopped, with no open PR and
//! nothing new for `stopped_after` (10 minutes). Table-driven: each case is one rule.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use airboss_attention::{AttentionConfig, AttentionMsg, Hidden, compute};
use airboss_core::{PrState, Status};
use airboss_services::{NeedsGroup, NeedsKind, Target, ThreadState, ThreadView};
use common::*;

const MIN: Duration = Duration::from_secs(60);
const MS: Duration = Duration::from_millis(1);

/// The task's own last event, and the moment its agent stopped: day 1, 10:00.
fn stop() -> String {
    ts(1, 10)
}

/// A thread in `state` whose last activity was `stop()`.
fn agent(r: &str, state: ThreadState) -> ThreadView {
    let mut t = thread(r, state);
    t.last_activity_at = Some(stop());
    t
}

fn task(status: Status) -> airboss_core::CoreSnapshot {
    Core::new()
        .titled(7, None, status, &ts(1, 9), "Web: slot picker")
        .build()
}

/// Day 1, 10:00 plus `m` minutes.
fn after(m: u32) -> std::time::SystemTime {
    at(1, 10) + MIN * m
}

fn stopped_keys(out: &airboss_attention::Computed) -> Vec<String> {
    out.items
        .iter()
        .filter(|i| i.kind == NeedsKind::AgentStopped)
        .map(|i| i.key.0.clone())
        .collect()
}

#[test]
fn the_default_wait_is_ten_minutes() {
    assert_eq!(AttentionConfig::default().stopped_after, MIN * 10);
}

#[test]
fn a_doing_task_whose_agent_stopped_with_no_pr_shows() {
    let sync = Links::new()
        .thread(agent("slot-picker", Idle), &[7])
        .build();
    let out = run(task(Doing), sync, after(10));
    assert_eq!(out.items.len(), 1);
    let item = &out.items[0];
    assert_eq!(item.key, key(&format!("stopped:7:{}", stop())));
    assert_eq!(item.kind, NeedsKind::AgentStopped);
    assert_eq!(item.group, NeedsGroup::Stopped);
    assert_eq!(item.task, id(7));
    assert_eq!(item.task_title, "Web: slot picker");
    assert_eq!(item.since, stop());
    assert_eq!(item.headline, "Agent stopped · no PR yet");
    assert_eq!(item.context, "slot-picker-thread stopped");
    assert_eq!(item.target, Target::Thread(t3("slot-picker")));
    assert!(item.proposed.is_empty());
    assert!(!item.stale);
}

/// One row per rule: what the inputs are, and whether Agent stopped shows.
#[test]
fn each_rule_decides_whether_it_shows() {
    struct Case {
        name: &'static str,
        status: Status,
        threads: Vec<ThreadView>,
        pr: Option<(Option<PrState>, bool)>,
        now: std::time::SystemTime,
        shows: bool,
    }
    let idle = || agent("a", Idle);
    let cases = [
        Case {
            name: "idle, no PR, 10 minutes",
            status: Doing,
            threads: vec![idle()],
            pr: None,
            now: after(10),
            shows: true,
        },
        Case {
            name: "settled",
            status: Doing,
            threads: vec![agent("a", Settled)],
            pr: None,
            now: after(10),
            shows: true,
        },
        Case {
            name: "gone",
            status: Doing,
            threads: vec![agent("a", Gone)],
            pr: None,
            now: after(10),
            shows: true,
        },
        Case {
            name: "idle, settled and gone together",
            status: Doing,
            threads: vec![idle(), agent("b", Settled), agent("c", Gone)],
            pr: None,
            now: after(10),
            shows: true,
        },
        Case {
            name: "one minute too early",
            status: Doing,
            threads: vec![idle()],
            pr: None,
            now: after(10) - MS,
            shows: false,
        },
        Case {
            name: "still running",
            status: Doing,
            threads: vec![agent("a", Running)],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "one of two still running",
            status: Doing,
            threads: vec![idle(), agent("b", Running)],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "waiting on me (a question shows instead)",
            status: Doing,
            threads: vec![idle(), agent("b", NeedsMe)],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "errored (a failed run shows instead)",
            status: Doing,
            threads: vec![idle(), agent("b", Errored)],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "no linked thread",
            status: Doing,
            threads: vec![],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "an open PR",
            status: Doing,
            threads: vec![idle()],
            pr: Some((Some(PrState::Open), false)),
            now: after(60),
            shows: false,
        },
        Case {
            name: "an open draft PR",
            status: Doing,
            threads: vec![idle()],
            pr: Some((Some(PrState::Open), true)),
            now: after(60),
            shows: false,
        },
        Case {
            name: "a PR whose state isn't known yet",
            status: Doing,
            threads: vec![idle()],
            pr: Some((None, false)),
            now: after(60),
            shows: false,
        },
        Case {
            name: "only a closed PR",
            status: Doing,
            threads: vec![idle()],
            pr: Some((Some(PrState::Closed), false)),
            now: after(60),
            shows: true,
        },
        Case {
            name: "todo",
            status: Todo,
            threads: vec![idle()],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "in review",
            status: InReview,
            threads: vec![idle()],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "blocked",
            status: Blocked,
            threads: vec![idle()],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "done",
            status: Done,
            threads: vec![idle()],
            pr: None,
            now: after(60),
            shows: false,
        },
        Case {
            name: "dropped",
            status: Dropped,
            threads: vec![idle()],
            pr: None,
            now: after(60),
            shows: false,
        },
    ];
    for c in cases {
        let mut links = Links::new();
        for t in c.threads {
            links = links.thread(t, &[7]);
        }
        if let Some((state, draft)) = c.pr {
            let mut p = pr(9);
            p.state = state;
            p.draft = draft;
            p.updated_at = Some(ts(0, 0));
            links = links.pr(p, &[7]);
        }
        let out = run(task(c.status), links.build(), c.now);
        assert_eq!(!stopped_keys(&out).is_empty(), c.shows, "{}", c.name);
    }
}

#[test]
fn a_closed_pr_says_no_open_pr() {
    let mut p = pr(9);
    p.state = Some(PrState::Closed);
    p.updated_at = Some(ts(0, 0));
    let sync = Links::new()
        .thread(agent("a", Idle), &[7])
        .pr(p, &[7])
        .build();
    let out = run(task(Doing), sync, after(10));
    assert_eq!(out.items[0].headline, "Agent stopped · no open PR");
}

#[test]
fn ten_minutes_count_from_the_latest_change_to_the_task_or_its_threads() {
    // The task changed at 10:05, after the agent stopped at 10:00.
    let core = Core::new()
        .task(7, None, Doing, &airboss_services::time::to_ts(after(5)))
        .build();
    let sync = || Links::new().thread(agent("a", Idle), &[7]).build();
    assert!(run(core.clone(), sync(), after(15) - MS).items.is_empty());
    let out = run(core.clone(), sync(), after(15));
    assert_eq!(out.items[0].since, airboss_services::time::to_ts(after(5)));

    // A second thread active until 10:08.
    let mut late = agent("b", Settled);
    late.last_activity_at = Some(airboss_services::time::to_ts(after(8)));
    let sync = Links::new()
        .thread(agent("a", Idle), &[7])
        .thread(late, &[7])
        .build();
    let out = run(task(Doing), sync, after(18));
    assert_eq!(out.items[0].since, airboss_services::time::to_ts(after(8)));
    assert_eq!(out.items[0].context, "All 2 threads stopped");
    assert_eq!(out.items[0].target, Target::Thread(t3("b")), "the latest");
}

#[test]
fn a_subtasks_change_doesnt_count_as_the_tasks_own() {
    let core = Core::new()
        .task(7, None, Doing, &ts(1, 9))
        .task(8, Some(7), Todo, &airboss_services::time::to_ts(after(9)))
        .build();
    let sync = Links::new().thread(agent("a", Idle), &[7]).build();
    assert_eq!(stopped_keys(&run(core, sync, after(10))).len(), 1);
}

#[test]
fn next_wake_is_when_the_ten_minutes_run_out() {
    let sync = || Links::new().thread(agent("a", Idle), &[7]).build();
    for now in [after(0), after(3), after(10) - MS] {
        let out = run(task(Doing), sync(), now);
        assert!(out.items.is_empty());
        assert_eq!(out.next_wake, Some(after(10)), "{now:?}");
    }
    // Once it shows, nothing more is due from it.
    let out = run(task(Doing), sync(), after(10));
    assert_eq!(out.items.len(), 1);
    assert!(out.next_wake.is_none_or(|w| w > after(10)));
}

#[test]
fn the_wait_comes_from_the_config() {
    let cfg = AttentionConfig {
        stopped_after: MIN * 30,
        ..AttentionConfig::default()
    };
    let input = inputs(
        task(Doing),
        Links::new().thread(agent("a", Idle), &[7]).build(),
    );
    let early = compute(&input, &[], after(29), &cfg);
    assert!(early.items.is_empty());
    assert_eq!(early.next_wake, Some(after(30)));
    assert_eq!(compute(&input, &[], after(30), &cfg).items.len(), 1);
}

#[test]
fn a_thread_on_two_doing_tasks_gives_each_its_own_item() {
    let core = Core::new()
        .task(7, None, Doing, &ts(1, 9))
        .task(9, None, Doing, &ts(1, 9))
        .build();
    let sync = Links::new().thread(agent("a", Idle), &[9, 7]).build();
    let out = run(core, sync, after(10));
    assert_eq!(
        stopped_keys(&out),
        vec![
            format!("stopped:7:{}", stop()),
            format!("stopped:9:{}", stop())
        ]
    );
}

#[test]
fn a_thread_without_a_title_names_the_task() {
    let mut t = agent("a", Idle);
    t.title = None;
    let sync = Links::new().thread(t, &[7]).build();
    let out = run(task(Doing), sync, after(10));
    assert_eq!(out.items[0].context, "Web: slot picker stopped");
}

#[test]
fn a_stale_thread_makes_a_stale_item() {
    let mut t = agent("a", Idle);
    t.stale = true;
    let sync = Links::new().thread(t, &[7]).build();
    assert!(run(task(Doing), sync, after(10)).items[0].stale);
}

#[test]
fn a_linked_thread_sync_has_no_view_of_yet_shows_nothing() {
    let mut sync = Links::new().thread(agent("a", Idle), &[7]).build();
    sync.threads.clear();
    assert!(run(task(Doing), sync, after(60)).items.is_empty());
}

#[test]
fn it_sorts_right_after_failed_runs() {
    let core = Core::new()
        .task(1, None, Doing, &ts(1, 9))
        .task(4, Some(1), Proposed, &ts(0, 0))
        .task(2, None, Doing, &ts(1, 9))
        .build();
    let mut failed = agent("f", Errored);
    failed.run_id = Some("run-1".into());
    let sync = Links::new()
        .thread(failed, &[1])
        .thread(agent("s", Idle), &[2])
        .build();
    let out = run(core, sync, after(10));
    let kinds: Vec<NeedsKind> = out.items.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        vec![
            NeedsKind::FailedRun,
            NeedsKind::AgentStopped,
            NeedsKind::Proposed
        ]
    );
}

#[test]
fn quiet_gives_way_to_a_visible_agent_stopped_item_on_the_same_task() {
    let sync = || Links::new().thread(agent("a", Idle), &[7]).build();
    let later = at(8, 0);
    let out = run(task(Doing), sync(), later);
    assert_eq!(
        out.items.iter().map(|i| i.kind).collect::<Vec<_>>(),
        vec![NeedsKind::AgentStopped]
    );
    // Dismissed, it leaves the reminder to Quiet.
    let hidden = [Hidden {
        key: key(&format!("stopped:7:{}", stop())),
        until: None,
    }];
    let out = run_hidden(task(Doing), sync(), &hidden, later);
    assert_eq!(
        out.items.iter().map(|i| i.kind).collect::<Vec<_>>(),
        vec![NeedsKind::Quiet]
    );
}

#[test]
fn a_new_stop_is_a_new_occurrence() {
    let hidden = [Hidden {
        key: key(&format!("stopped:7:{}", stop())),
        until: None,
    }];
    let sync = Links::new().thread(agent("a", Idle), &[7]).build();
    assert!(
        run_hidden(task(Doing), sync, &hidden, after(10))
            .items
            .is_empty()
    );
    // The agent was resumed and stopped again at 11:00.
    let mut again = agent("a", Idle);
    again.last_activity_at = Some(ts(1, 11));
    let sync = Links::new().thread(again, &[7]).build();
    let out = run_hidden(task(Doing), sync, &hidden, at(1, 11) + MIN * 10);
    assert_eq!(stopped_keys(&out), vec![format!("stopped:7:{}", ts(1, 11))]);
}

#[test]
fn the_worker_shows_it_on_the_tick_at_the_deadline_and_on_wake() {
    let sync = Links::new().thread(agent("a", Idle), &[7]).build();
    let (mut w, _) = worker(task(Doing), sync.clone(), after(1));
    assert_eq!(w.needs_me().count, 0);
    assert_eq!(
        w.handle(AttentionMsg::Recheck, after(2)),
        airboss_attention::Step::Wait(after(10))
    );
    w.handle(AttentionMsg::Tick, after(10));
    assert_eq!(w.needs_me().count, 1);

    // The Mac slept through the moment: `woke()` puts it there at once.
    let (mut w, _) = worker(task(Doing), sync, after(1));
    w.handle(AttentionMsg::Woke, after(300));
    assert_eq!(w.needs_me().count, 1);
}

#[test]
fn it_clears_when_the_agent_runs_again_or_the_task_moves_on() {
    let sync = Links::new().thread(agent("a", Running), &[7]).build();
    assert!(run(task(Doing), sync, after(30)).items.is_empty());
    for status in [InReview, Done] {
        let sync = Links::new().thread(agent("a", Idle), &[7]).build();
        assert!(run(task(status), sync, after(30)).items.is_empty());
    }
}
