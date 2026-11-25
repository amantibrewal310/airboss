//! Every kind through `compute` alone: one test per kind showing, and one per rule hiding it.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use airboss_attention::AttentionConfig;
use common::*;

#[test]
fn the_defaults_are_the_em_thresholds() {
    let cfg = AttentionConfig::default();
    assert_eq!(cfg.quiet_after, DAY * 5);
    assert_eq!(cfg.snooze_for, DAY * 3);
    assert_eq!(cfg.ready_window, DAY * 7);
    assert_eq!(cfg.menu_items, 6);
    assert_eq!(cfg.settle, Duration::from_millis(150));
}

#[test]
fn nothing_in_gives_nothing_out() {
    let out = run(empty_core(), Links::new().build(), at(10, 0));
    assert!(out.items.is_empty());
    assert!(out.live_keys.is_empty());
    assert_eq!(out.next_wake, None);
}

// ---------- Questions and approvals ----------

use airboss_services::{NeedsGroup, NeedsKind, RequestKind, Target};

fn asking(r: &str, kind: RequestKind, text: &str) -> airboss_services::ThreadView {
    let mut t = thread(r, NeedsMe);
    t.request = Some(request(&format!("req-{r}"), kind, text, &ts(2, 3)));
    t
}

#[test]
fn a_question_in_a_linked_thread_shows() {
    let core = Core::new()
        .titled(5, None, Doing, &ts(1, 0), "Web: slot picker")
        .build();
    let sync = Links::new()
        .thread(
            asking(
                "a1",
                RequestKind::Question,
                "Should slots be 15 or 30 minutes?",
            ),
            &[5],
        )
        .build();
    let out = run(core, sync, at(3, 0));
    assert_eq!(out.items.len(), 1);
    let item = &out.items[0];
    assert_eq!(item.key, key("question:req-a1"));
    assert_eq!(item.kind, NeedsKind::Question);
    assert_eq!(item.group, NeedsGroup::Questions);
    assert_eq!(item.task, id(5));
    assert_eq!(item.task_title, "Web: slot picker");
    assert_eq!(item.since, ts(2, 3));
    assert_eq!(item.headline, "Should slots be 15 or 30 minutes?");
    assert_eq!(item.context, "Agent in a1-thread asks");
    assert_eq!(item.target, Target::Thread(t3("a1")));
    assert!(item.proposed.is_empty());
    assert!(!item.stale);
    assert_eq!(out.live_keys, vec![key("question:req-a1")]);
}

#[test]
fn an_approval_shows_its_first_line() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new()
        .thread(
            asking(
                "a1",
                RequestKind::Approval,
                "  cargo add tray-icon\nin crates/app",
            ),
            &[5],
        )
        .build();
    let out = run(core, sync, at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key("approval:req-a1"));
    assert_eq!(item.kind, NeedsKind::Approval);
    assert_eq!(item.group, NeedsGroup::Questions);
    assert_eq!(item.headline, "Allow: cargo add tray-icon");
    assert_eq!(item.context, "Agent in a1-thread wants approval");
}

#[test]
fn a_question_on_a_closed_task_is_hidden() {
    for status in [Done, Dropped, Proposed] {
        let core = Core::new().task(5, None, status, &ts(1, 0)).build();
        let sync = Links::new()
            .thread(asking("a1", RequestKind::Question, "Q?"), &[5])
            .build();
        let out = run(core, sync, at(3, 0));
        assert!(
            out.items.iter().all(|i| i.kind != NeedsKind::Question),
            "{status:?}"
        );
    }
}

#[test]
fn every_open_status_shows_a_question() {
    for status in [Todo, Doing, InReview, Blocked] {
        let core = Core::new().task(5, None, status, &ts(1, 0)).build();
        let sync = Links::new()
            .thread(asking("a1", RequestKind::Question, "Q?"), &[5])
            .build();
        assert_eq!(run(core, sync, at(3, 0)).items.len(), 1, "{status:?}");
    }
}

#[test]
fn a_waiting_thread_without_a_request_shows_nothing() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new().thread(thread("a1", NeedsMe), &[5]).build();
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn an_untracked_thread_never_shows() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let mut sync = Links::new().build();
    let mut t = asking("u1", RequestKind::Question, "Q?");
    t.linked = false;
    sync.threads.insert(t.key.clone(), t);
    let mut e = thread("u2", Errored);
    e.linked = false;
    sync.threads.insert(e.key.clone(), e);
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn a_thread_on_several_tasks_shows_once_at_the_lowest_open_task() {
    let core = Core::new()
        .task(4, None, Done, &ts(1, 0))
        .task(7, None, Doing, &ts(1, 0))
        .task(9, None, Doing, &ts(1, 0))
        .build();
    let sync = Links::new()
        .thread(asking("a1", RequestKind::Question, "Q?"), &[9, 4, 7])
        .build();
    let out = run(core, sync, at(3, 0));
    assert_eq!(keys(&out), vec!["question:req-a1"]);
    assert_eq!(out.items[0].task, id(7));
}

#[test]
fn a_thread_without_a_title_names_its_task() {
    let core = Core::new()
        .titled(5, None, Doing, &ts(1, 0), "Tray count")
        .build();
    let mut t = asking("a1", RequestKind::Question, "Q?");
    t.title = None;
    let sync = Links::new().thread(t, &[5]).build();
    assert_eq!(
        run(core, sync, at(3, 0)).items[0].context,
        "Agent in Tray count asks"
    );
}

#[test]
fn an_item_from_a_stale_thread_is_stale() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let mut t = asking("a1", RequestKind::Question, "Q?");
    t.stale = true;
    let sync = Links::new().thread(t, &[5]).build();
    assert!(run(core, sync, at(3, 0)).items[0].stale);
}

// ---------- Failed runs ----------

fn failed(r: &str, run: Option<&str>, error: Option<&str>) -> airboss_services::ThreadView {
    let mut t = thread(r, Errored);
    t.run_id = run.map(str::to_string);
    t.error = error.map(str::to_string);
    t.last_activity_at = Some(ts(2, 5));
    t
}

#[test]
fn a_failed_run_shows() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new()
        .thread(
            failed("fix-flaky-login", Some("run-9"), Some("3 tests failed")),
            &[5],
        )
        .build();
    let out = run(core, sync, at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key("failed:run-9"));
    assert_eq!(item.kind, NeedsKind::FailedRun);
    assert_eq!(item.group, NeedsGroup::FailedRuns);
    assert_eq!(item.since, ts(2, 5));
    assert_eq!(
        item.headline,
        "fix-flaky-login-thread stopped: 3 tests failed"
    );
    assert_eq!(item.context, "Thread stopped · 3 tests failed");
    assert_eq!(item.target, Target::Thread(t3("fix-flaky-login")));
}

#[test]
fn a_failed_run_without_an_error_or_run_id_still_shows() {
    let core = Core::new().task(5, None, Doing, &ts(1, 0)).build();
    let sync = Links::new().thread(failed("a1", None, None), &[5]).build();
    let item = run(core, sync, at(3, 0)).items.remove(0);
    assert_eq!(item.key, key(&format!("failed:a1@{}", ts(2, 5))));
    assert_eq!(item.headline, "a1-thread stopped");
    assert_eq!(item.context, "Thread stopped");
}

#[test]
fn a_failed_run_on_a_closed_task_is_hidden() {
    let core = Core::new().task(5, None, Done, &ts(1, 0)).build();
    let sync = Links::new()
        .thread(failed("a1", Some("r"), Some("boom")), &[5])
        .build();
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn threads_that_are_running_idle_settled_or_gone_ask_nothing() {
    for state in [Running, Idle, Settled, Gone] {
        let core = Core::new().task(5, None, Todo, &ts(1, 0)).build();
        let sync = Links::new().thread(thread("a1", state), &[5]).build();
        assert!(run(core, sync, at(1, 1)).items.is_empty(), "{state:?}");
    }
}

// ---------- Reviews ----------

use airboss_core::PrState;
use airboss_services::{Checks, PrProblem, Review};

fn review_core() -> airboss_core::CoreSnapshot {
    Core::new().task(5, None, InReview, &ts(1, 0)).build()
}

#[test]
fn an_open_pr_ready_for_review_shows() {
    let mut p = pr(58);
    p.ready_since = Some("2026-11-02T04:00:00Z".into()); // GitHub's form, no fraction
    let sync = Links::new().pr(p, &[5]).build();
    let out = run(review_core(), sync, at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key("review:acme/web#58"));
    assert_eq!(item.kind, NeedsKind::Review);
    assert_eq!(item.group, NeedsGroup::Reviews);
    assert_eq!(item.since, ts(1, 4));
    assert_eq!(item.headline, "PR #58 · Change 58");
    assert_eq!(item.context, "+10 −2 · checks passing · review required");
    assert_eq!(
        item.target,
        Target::Pr {
            r#ref: pr_ref(58),
            url: Some("https://github.com/acme/web/pull/58".into())
        }
    );
}

#[test]
fn a_reviews_context_says_what_github_said() {
    let cases = [
        (
            Checks::Failing,
            Review::ChangesRequested,
            "+10 −2 · checks failing · changes requested",
        ),
        (
            Checks::Pending,
            Review::Approved,
            "+10 −2 · checks pending · approved",
        ),
        (Checks::None, Review::None, "+10 −2"),
    ];
    for (checks, review, context) in cases {
        let mut p = pr(1);
        p.checks = checks;
        p.review = review;
        let out = run(review_core(), Links::new().pr(p, &[5]).build(), at(3, 0));
        assert_eq!(out.items[0].context, context);
    }
    let mut p = pr(1);
    p.additions = None;
    p.title = None;
    let out = run(review_core(), Links::new().pr(p, &[5]).build(), at(3, 0));
    assert_eq!(out.items[0].headline, "PR #1");
    assert_eq!(out.items[0].context, "checks passing · review required");
}

#[test]
fn a_pr_that_isnt_ready_for_review_is_hidden() {
    let mut draft = pr(1);
    draft.draft = true;
    let mut merged = pr(2);
    merged.state = Some(PrState::Merged);
    let mut closed = pr(3);
    closed.state = Some(PrState::Closed);
    let mut unread = pr(4);
    unread.state = None;
    let mut broken = pr(5);
    broken.problem = Some(PrProblem::NoAccess);
    for p in [draft, merged, closed, unread, broken] {
        let r = p.r#ref.clone();
        let out = run(review_core(), Links::new().pr(p, &[5]).build(), at(3, 0));
        assert!(out.items.is_empty(), "{r}");
    }
}

#[test]
fn a_pr_on_a_closed_task_is_hidden() {
    let core = Core::new().task(5, None, Done, &ts(1, 0)).build();
    assert!(
        run(core, Links::new().pr(pr(1), &[5]).build(), at(3, 0))
            .items
            .is_empty()
    );
}

#[test]
fn a_pr_isnt_a_review_while_an_agent_on_its_task_runs_or_waits() {
    for state in [Running, NeedsMe] {
        let sync = Links::new()
            .pr(pr(1), &[5])
            .thread(thread("a1", state), &[5])
            .build();
        let out = run(review_core(), sync, at(3, 0));
        assert!(
            out.items.iter().all(|i| i.kind != NeedsKind::Review),
            "{state:?}"
        );
    }
    // Once the agent stops, it's a review.
    let sync = Links::new()
        .pr(pr(1), &[5])
        .thread(thread("a1", Idle), &[5])
        .build();
    assert_eq!(
        keys(&run(review_core(), sync, at(3, 0))),
        vec!["review:acme/web#1"]
    );
}

#[test]
fn an_agent_on_any_of_the_prs_tasks_holds_the_review() {
    let core = Core::new()
        .task(5, None, InReview, &ts(1, 0))
        .task(6, None, Doing, &ts(1, 0))
        .build();
    let sync = Links::new()
        .pr(pr(1), &[5, 6])
        .thread(thread("a1", Running), &[6])
        .build();
    assert!(run(core, sync, at(3, 0)).items.is_empty());
}

#[test]
fn a_pr_on_several_tasks_shows_once_at_the_lowest_open_task() {
    let core = Core::new()
        .task(3, None, Done, &ts(1, 0))
        .task(5, None, InReview, &ts(1, 0))
        .task(8, None, InReview, &ts(1, 0))
        .build();
    let out = run(core, Links::new().pr(pr(1), &[8, 3, 5]).build(), at(3, 0));
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].task, id(5));
}

#[test]
fn a_review_without_ready_since_uses_its_last_update() {
    let mut p = pr(1);
    p.ready_since = None;
    p.updated_at = Some(ts(2, 2));
    p.stale = true;
    let out = run(review_core(), Links::new().pr(p, &[5]).build(), at(3, 0));
    assert_eq!(out.items[0].since, ts(2, 2));
    assert!(out.items[0].stale);
}

// ---------- Proposed ----------

#[test]
fn proposals_under_one_parent_are_one_item() {
    let core = Core::new()
        .titled(150, None, Doing, &ts(1, 0), "Clean up worktrees")
        .titled(152, Some(150), Proposed, &ts(2, 0), "Find merged branches")
        .created(&ts(2, 0))
        .titled(151, Some(150), Proposed, &ts(2, 1), "List stale worktrees")
        .created(&ts(2, 1))
        .titled(153, Some(150), Todo, &ts(1, 0), "Already accepted")
        .build();
    let out = run(core, Links::new().build(), at(3, 0));
    assert_eq!(out.items.len(), 1);
    let item = &out.items[0];
    assert_eq!(item.key, key("proposed:150:152"));
    assert_eq!(item.kind, NeedsKind::Proposed);
    assert_eq!(item.group, NeedsGroup::Proposed);
    assert_eq!(item.task, id(150));
    assert_eq!(item.task_title, "Clean up worktrees");
    assert_eq!(item.since, ts(2, 1));
    assert_eq!(item.headline, "The agent proposed 2 subtasks");
    assert_eq!(item.context, "2 proposed subtasks");
    assert_eq!(item.target, Target::Task(id(150)));
    assert_eq!(
        item.proposed,
        vec![
            (id(151), "List stale worktrees".to_string()),
            (id(152), "Find merged branches".to_string())
        ]
    );
}

#[test]
fn a_new_proposal_under_the_same_parent_is_a_new_occurrence() {
    let core = Core::new()
        .task(1, None, Doing, &ts(1, 0))
        .task(2, Some(1), Proposed, &ts(1, 0))
        .task(9, Some(1), Proposed, &ts(1, 0))
        .build();
    assert_eq!(
        keys(&run(core, Links::new().build(), at(3, 0))),
        vec!["proposed:1:9"]
    );
}

#[test]
fn one_proposed_subtask_reads_singular() {
    let core = Core::new()
        .task(1, None, Doing, &ts(1, 0))
        .task(2, Some(1), Proposed, &ts(1, 0))
        .build();
    let item = run(core, Links::new().build(), at(3, 0)).items.remove(0);
    assert_eq!(item.headline, "The agent proposed 1 subtask");
    assert_eq!(item.context, "1 proposed subtask");
}

#[test]
fn a_top_level_proposal_is_its_own_item() {
    let core = Core::new()
        .titled(7, None, Proposed, &ts(1, 0), "Add a dark mode")
        .created(&ts(1, 2))
        .titled(8, None, Proposed, &ts(1, 0), "Add a light mode")
        .build();
    let out = run(core, Links::new().build(), at(3, 0));
    assert_eq!(keys(&out), vec!["proposed:7", "proposed:8"]);
    let item = &out.items[0];
    assert_eq!(item.task, id(7));
    assert_eq!(item.since, ts(1, 2));
    assert_eq!(item.headline, "The agent proposed a task");
    assert_eq!(item.context, "Proposed task");
    assert_eq!(item.proposed, vec![(id(7), "Add a dark mode".to_string())]);
}

// ---------- Ready ----------

#[test]
fn a_task_whose_blocker_finished_is_ready() {
    let core = Core::new()
        .task(143, None, Done, &ts(1, 0))
        .titled(146, None, Todo, &ts(1, 0), "Ship it")
        .edge(146, 143)
        .ready(146, &ts(2, 0))
        .build();
    let out = run(core, Links::new().build(), at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key(&format!("ready:146:{}", ts(2, 0))));
    assert_eq!(item.kind, NeedsKind::Ready);
    assert_eq!(item.group, NeedsGroup::Ready);
    assert_eq!(item.task, id(146));
    assert_eq!(item.task_title, "Ship it");
    assert_eq!(item.since, ts(2, 0));
    assert_eq!(item.headline, "Ready to start: 143 is done");
    assert_eq!(item.context, "143 done · ready to start");
    assert_eq!(item.target, Target::Task(id(146)));
}

#[test]
fn ready_names_two_blockers_in_id_order_and_counts_more() {
    let two = Core::new()
        .task(145, None, Done, &ts(1, 0))
        .task(143, None, Dropped, &ts(1, 0))
        .task(146, None, Todo, &ts(1, 0))
        .edge(146, 145)
        .edge(146, 143)
        .ready(146, &ts(2, 0))
        .build();
    let item = run(two, Links::new().build(), at(3, 0)).items.remove(0);
    assert_eq!(item.headline, "Ready to start: 143 and 145 are done");
    assert_eq!(item.context, "143 and 145 done · ready to start");

    let three = Core::new()
        .task(1, None, Done, &ts(1, 0))
        .task(2, None, Done, &ts(1, 0))
        .task(3, None, Done, &ts(1, 0))
        .task(4, None, Todo, &ts(1, 0))
        .edge(4, 1)
        .edge(4, 2)
        .edge(4, 3)
        .ready(4, &ts(2, 0))
        .build();
    let item = run(three, Links::new().build(), at(3, 0)).items.remove(0);
    assert_eq!(
        item.headline,
        "Ready to start: 3 tasks it waited on are done"
    );
    assert_eq!(item.context, "3 tasks done · ready to start");
}

#[test]
fn ready_says_merged_when_a_blockers_pr_merged() {
    let core = || {
        Core::new()
            .task(143, None, Done, &ts(1, 0))
            .task(145, None, Done, &ts(1, 0))
            .task(146, None, Todo, &ts(1, 0))
            .edge(146, 143)
            .ready(146, &ts(2, 0))
    };
    let mut merged = pr(58);
    merged.state = Some(PrState::Merged);
    let sync = || Links::new().pr(merged.clone(), &[143]).build();
    let item = run(core().build(), sync(), at(3, 0)).items.remove(0);
    assert_eq!(item.headline, "Ready to start: 143 is done");
    assert_eq!(item.context, "143 merged · ready to start");

    let item = run(core().edge(146, 145).build(), sync(), at(3, 0))
        .items
        .remove(0);
    assert_eq!(item.context, "143 merged and 145 done · ready to start");

    // An open PR on the blocker isn't "merged".
    let item = run(
        core().build(),
        Links::new().pr(pr(59), &[143]).build(),
        at(3, 0),
    )
    .items
    .remove(0);
    assert_eq!(item.context, "143 done · ready to start");
}

#[test]
fn a_ready_task_that_never_waited_isnt_news() {
    let core = Core::new()
        .task(146, None, Todo, &ts(1, 0))
        .ready(146, &ts(2, 0))
        .build();
    assert!(run(core, Links::new().build(), at(3, 0)).items.is_empty());
}

#[test]
fn readiness_older_than_the_window_is_hidden() {
    let core = || {
        Core::new()
            .task(1, None, Done, &ts(0, 0))
            .task(2, None, Todo, &ts(0, 0))
            .edge(2, 1)
            .ready(2, &ts(1, 0))
            .build()
    };
    // Just inside 7 days, then exactly 7 days.
    let inside = run(
        core(),
        Links::new().build(),
        at(8, 0) - Duration::from_millis(1),
    );
    assert_eq!(inside.items.len(), 1);
    assert!(run(core(), Links::new().build(), at(8, 0)).items.is_empty());
}

// ---------- Finish it? ----------

#[test]
fn an_open_parent_with_every_subtask_done_asks_to_finish() {
    let core = Core::new()
        .titled(110, None, Doing, &ts(1, 0), "Menu bar")
        .task(111, Some(110), Done, &ts(2, 0))
        .task(112, Some(110), Done, &ts(2, 4))
        .task(113, Some(110), Dropped, &ts(2, 9))
        .build();
    let out = run(core, Links::new().build(), at(3, 0));
    let item = &out.items[0];
    assert_eq!(item.key, key(&format!("finish:110:{}", ts(2, 4))));
    assert_eq!(item.kind, NeedsKind::FinishIt);
    assert_eq!(item.group, NeedsGroup::FinishIt);
    assert_eq!(item.task, id(110));
    assert_eq!(item.since, ts(2, 4));
    assert_eq!(
        item.headline,
        "All 2 subtasks are done. Mark the parent done?"
    );
    assert_eq!(item.context, "All 2 subtasks done");
    assert_eq!(item.target, Target::Task(id(110)));
}

#[test]
fn finish_it_is_hidden_unless_every_kept_subtask_is_done() {
    let cases: [(&str, Vec<airboss_core::Status>); 4] = [
        ("an open subtask", vec![Done, Doing]),
        ("a proposed subtask", vec![Done, Proposed]),
        ("only dropped subtasks", vec![Dropped, Dropped]),
        ("no subtasks", vec![]),
    ];
    for (why, kids) in cases {
        let mut core = Core::new().task(1, None, Doing, &ts(1, 0));
        for (i, s) in kids.into_iter().enumerate() {
            core = core.task(10 + i as i64, Some(1), s, &ts(1, 0));
        }
        let out = run(core.build(), Links::new().build(), at(3, 0));
        assert!(
            out.items.iter().all(|i| i.kind != NeedsKind::FinishIt),
            "{why}"
        );
    }
}

#[test]
fn a_closed_parent_isnt_asked_to_finish() {
    for status in [Done, Dropped, Proposed] {
        let core = Core::new()
            .task(1, None, status, &ts(1, 0))
            .task(2, Some(1), Done, &ts(1, 0))
            .build();
        let out = run(core, Links::new().build(), at(3, 0));
        assert!(
            out.items.iter().all(|i| i.kind != NeedsKind::FinishIt),
            "{status:?}"
        );
    }
}

#[test]
fn finish_it_counts_direct_subtasks_only() {
    // 1's only child is 2, which is done; 3 under 2 doesn't count for 1.
    let core = Core::new()
        .task(1, None, Doing, &ts(1, 0))
        .task(2, Some(1), Done, &ts(1, 0))
        .task(3, Some(2), Done, &ts(1, 0))
        .build();
    let out = run(core, Links::new().build(), at(3, 0));
    let item = out.items.iter().find(|i| i.task == id(1)).unwrap();
    assert_eq!(item.headline, "1 subtask is done. Mark the parent done?");
    assert_eq!(item.context, "1 subtask done");
}
