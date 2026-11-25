//! The design's order: groups, newest first, Quiet longest first, ties.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::PrState;
use airboss_services::{NeedsGroup, NeedsKind, RequestKind};
use common::*;

fn asking(r: &str, req: &str, at: &str) -> airboss_services::ThreadView {
    let mut t = thread(r, NeedsMe);
    t.request = Some(request(req, RequestKind::Question, "Q?", at));
    t
}

#[test]
fn groups_come_in_the_designs_order() {
    let core = Core::new()
        // Quiet
        .task(1, None, Doing, &ts(0, 0))
        // A parent with a done subtask
        .task(2, None, Todo, &ts(9, 0))
        .task(3, Some(2), Done, &ts(9, 0))
        // Ready
        .task(4, None, Done, &ts(9, 0))
        .task(5, None, Todo, &ts(9, 0))
        .edge(5, 4)
        .ready(5, &ts(9, 0))
        // Proposed (under 2)
        .task(6, Some(2), Proposed, &ts(9, 0))
        // Failed run, review, approval and question
        .task(7, None, Todo, &ts(9, 0))
        .task(8, None, InReview, &ts(9, 0))
        .task(9, None, Todo, &ts(9, 0))
        .build();
    let mut approval = thread("ap", NeedsMe);
    approval.request = Some(request("r2", RequestKind::Approval, "rm", &ts(9, 1)));
    let sync = Links::new()
        .thread(thread("f", Errored), &[7])
        .pr(pr(1), &[8])
        .thread(asking("q", "r1", &ts(9, 2)), &[9])
        .thread(approval, &[9])
        .build();
    let out = run(core, sync, at(10, 0));
    let kinds: Vec<NeedsKind> = out.items.iter().map(|i| i.kind).collect();
    // Task 2's proposed child keeps it from "Finish it?"; the next test places that group.
    assert_eq!(
        kinds,
        vec![
            NeedsKind::Question,
            NeedsKind::Approval,
            NeedsKind::Review,
            NeedsKind::FailedRun,
            NeedsKind::Proposed,
            NeedsKind::Ready,
            NeedsKind::Quiet,
        ]
    );
    let groups: Vec<NeedsGroup> = out.items.iter().map(|i| i.group).collect();
    let mut sorted = groups.clone();
    sorted.sort();
    assert_eq!(groups, sorted);
}

#[test]
fn finish_it_sits_between_ready_and_quiet() {
    let core = Core::new()
        .task(1, None, Doing, &ts(0, 0))
        .task(2, None, Todo, &ts(9, 0))
        .task(3, Some(2), Done, &ts(9, 0))
        .task(4, None, Done, &ts(9, 0))
        .task(5, None, Todo, &ts(9, 0))
        .edge(5, 4)
        .ready(5, &ts(9, 0))
        .build();
    let kinds: Vec<NeedsKind> = run(core, Links::new().build(), at(10, 0))
        .items
        .iter()
        .map(|i| i.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![NeedsKind::Ready, NeedsKind::FinishIt, NeedsKind::Quiet]
    );
}

#[test]
fn newest_first_inside_a_group() {
    let core = Core::new()
        .task(1, None, Todo, &ts(0, 0))
        .task(2, None, Todo, &ts(0, 0))
        .task(3, None, Todo, &ts(0, 0))
        .build();
    let sync = Links::new()
        .thread(asking("a", "old", &ts(1, 0)), &[1])
        .thread(asking("b", "new", &ts(3, 0)), &[2])
        .thread(asking("c", "mid", &ts(2, 0)), &[3])
        .build();
    assert_eq!(
        keys(&run(core, sync, at(4, 0))),
        vec!["question:new", "question:mid", "question:old"]
    );
}

#[test]
fn newest_first_compares_times_not_text() {
    // GitHub's "…:00Z" would sort after "…:00.500Z" as text; as a time it's earlier.
    let core = Core::new()
        .task(1, None, InReview, &ts(0, 0))
        .task(2, None, InReview, &ts(0, 0))
        .build();
    let mut a = pr(1);
    a.ready_since = Some("2026-11-02T00:00:00Z".into());
    let mut b = pr(2);
    b.ready_since = Some("2026-11-02T00:00:00.500Z".into());
    let sync = Links::new().pr(a, &[1]).pr(b, &[2]).build();
    assert_eq!(
        keys(&run(core, sync, at(4, 0))),
        vec!["review:acme/web#2", "review:acme/web#1"]
    );
}

#[test]
fn longest_quiet_first() {
    let core = Core::new()
        .task(1, None, Doing, &ts(2, 0))
        .task(2, None, Doing, &ts(0, 0))
        .task(3, None, Doing, &ts(1, 0))
        .build();
    let tasks: Vec<i64> = run(core, Links::new().build(), at(9, 0))
        .items
        .iter()
        .map(|i| i.task.0)
        .collect();
    assert_eq!(tasks, vec![2, 3, 1]);
}

#[test]
fn ties_go_by_task_id_then_key() {
    let core = Core::new()
        .task(2, None, Todo, &ts(0, 0))
        .task(3, None, Todo, &ts(0, 0))
        .build();
    let sync = Links::new()
        .thread(asking("a", "z", &ts(1, 0)), &[3])
        .thread(asking("b", "y", &ts(1, 0)), &[2])
        .thread(asking("c", "x", &ts(1, 0)), &[2])
        .build();
    assert_eq!(
        keys(&run(core, sync, at(4, 0))),
        vec!["question:x", "question:y", "question:z"]
    );
}

#[test]
fn the_order_never_depends_on_hash_order() {
    // Many links in hash maps: the list must come out the same every time.
    let mut core = Core::new();
    let mut links = Links::new();
    for n in 1..=40 {
        core = core.task(n, None, InReview, &ts(0, 0));
        let mut p = pr(n as u64);
        p.ready_since = Some(ts(1, (n % 3) as u64));
        if n % 5 == 0 {
            p.state = Some(PrState::Merged);
        }
        links = links.pr(p, &[n]);
    }
    let core = core.build();
    let sync = links.build();
    let first = keys(&run(core.clone(), sync.clone(), at(2, 0)));
    for _ in 0..5 {
        assert_eq!(keys(&run(core.clone(), sync.clone(), at(2, 0))), first);
    }
}

#[test]
fn hidden_items_leave_the_list_but_stay_live() {
    let core = Core::new()
        .task(1, None, Doing, &ts(0, 0))
        .task(2, None, Doing, &ts(1, 0))
        .build();
    let quiet1 = key(&format!("quiet:1:{}", ts(0, 0)));
    let hidden = [airboss_attention::Hidden {
        key: quiet1.clone(),
        until: None,
    }];
    let out = run_hidden(core, Links::new().build(), &hidden, at(9, 0));
    assert_eq!(keys(&out), vec![format!("quiet:2:{}", ts(1, 0))]);
    assert!(out.live_keys.contains(&quiet1));
    assert_eq!(out.live_keys.len(), 2);
}
