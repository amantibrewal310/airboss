//! The view model: rows, progress, repos, chips, waits-on, diff and seq; plus the run map.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use airboss_app::dto::{ChipDto, NextUp, Progress, StatusDto, WaveDot};
use airboss_app::runmap::{display_waves, map_fits, run_map};
use airboss_app::viewmodel::{ViewModel, diff, plan_cards};
use airboss_core::{Actor, NewTask, Status, Store, TaskEdit, TaskId};
use airboss_services::{
    NeedsKind, NeedsList, Source, SyncSnapshot, ThreadKey, ThreadState, ThreadView,
};
use common::{TestApp, needs_item};

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn add(s: &mut Store, title: &str, parent: Option<TaskId>) -> TaskId {
    let mut n = NewTask::new(title);
    n.parent = parent;
    s.add_task(n, &Actor::Me).unwrap().id
}

fn status(s: &mut Store, id: TaskId, to: Status) {
    let note = (to == Status::Blocked).then_some("stuck");
    s.set_status(id, to, note, &Actor::Me).unwrap();
}

fn vm(s: &Store) -> ViewModel {
    vm_with(s, SyncSnapshot::default(), NeedsList::default())
}

fn vm_with(s: &Store, sync: SyncSnapshot, needs: NeedsList) -> ViewModel {
    ViewModel::build(
        Arc::new(s.snapshot().unwrap()),
        s.repos().unwrap(),
        Arc::new(sync),
        Arc::new(needs),
        common::at(common::NOW),
    )
}

#[test]
fn progress_counts_leaves_only_leaves_dropped_out_and_counts_proposed_as_todo() {
    let mut s = store();
    let p = add(&mut s, "Parent", None);
    let a = add(&mut s, "A", Some(p));
    let _b = add(&mut s, "B", Some(p));
    let c = add(&mut s, "C", Some(p));
    let d = add(&mut s, "D", Some(p));
    let d1 = add(&mut s, "D1", Some(d));
    let d2 = add(&mut s, "D2", Some(d));
    let d3 = add(&mut s, "D3", Some(d));
    s.add_task(NewTask::new("E").under(p), &Actor::Agent("t3/x".into()))
        .unwrap();
    status(&mut s, a, Status::Done);
    status(&mut s, c, Status::Dropped);
    status(&mut s, d1, Status::Doing);
    status(&mut s, d2, Status::InReview);
    status(&mut s, d3, Status::Blocked);

    let v = vm(&s);
    assert_eq!(
        v.rows[&p].progress,
        Some(Progress {
            done: 1,
            total: 6, // A, B, D1, D2, D3 and the proposed E; not C, and not D itself
            in_review: 1,
            doing: 1,
            blocked: 1,
        })
    );
    assert_eq!(v.rows[&d].progress.unwrap().total, 3);
    assert_eq!(v.rows[&a].progress, None, "a leaf has no progress");
    assert!(v.rows[&p].has_children && !v.rows[&a].has_children);
}

#[test]
fn a_parent_shows_its_subtasks_repo_union_flagged_and_children_inherit_its_branch() {
    let mut s = store();
    let web = s
        .add_repo("web", Path::new("/r/web"), None, &Actor::Me)
        .unwrap()
        .id;
    let api = s
        .add_repo("api", Path::new("/r/api"), None, &Actor::Me)
        .unwrap()
        .id;
    let p = add(&mut s, "Parent", None);
    let a = s
        .add_task(NewTask::new("A").under(p).in_repo(web), &Actor::Me)
        .unwrap()
        .id;
    let b = s
        .add_task(NewTask::new("B").under(p).in_repo(api), &Actor::Me)
        .unwrap()
        .id;
    let lone = add(&mut s, "Lone", None);
    s.set_branch(p, Some("task-1-parent"), &Actor::Me).unwrap();

    let v = vm(&s);
    // Sorted by repo name, like core's own lists.
    assert_eq!(v.rows[&p].repos, vec![api, web]);
    assert!(v.rows[&p].repos_from_subtasks);
    assert_eq!(v.rows[&a].repos, vec![web]);
    assert!(!v.rows[&a].repos_from_subtasks);
    assert!(v.rows[&lone].repos.is_empty() && !v.rows[&lone].repos_from_subtasks);
    assert_eq!(v.rows[&b].branch.as_deref(), Some("task-1-parent"));
    assert_eq!(v.rows[&lone].branch, None);
    assert_eq!(v.repo_dtos[0].name, "api");
    assert_eq!(
        v.repo_dtos.iter().map(|r| r.color).collect::<Vec<_>>(),
        vec![2, 1]
    );
}

fn thread(id: &str, state: ThreadState) -> ThreadView {
    ThreadView {
        key: ThreadKey {
            source: Source::T3,
            r#ref: id.into(),
        },
        linked: true,
        state,
        title: Some(id.into()),
        request: None,
        error: None,
        run_id: None,
        project_path: None,
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: None,
        stale: false,
    }
}

fn sync_with(threads: &[(&str, ThreadState, TaskId)]) -> SyncSnapshot {
    let mut snap = SyncSnapshot::default();
    for (id, state, task) in threads {
        let t = thread(id, *state);
        snap.thread_tasks.insert(t.key.clone(), vec![*task]);
        snap.threads.insert(t.key.clone(), t);
    }
    snap
}

#[test]
fn the_live_chip_is_the_strongest_of_the_tasks_threads_while_it_is_open() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    let c = add(&mut s, "C", None);
    let d = add(&mut s, "D", None);
    status(&mut s, d, Status::Done);
    let sync = sync_with(&[
        ("a1", ThreadState::Running, a),
        ("a2", ThreadState::NeedsMe, a),
        ("a3", ThreadState::Idle, a),
        ("b1", ThreadState::Settled, b),
        ("b2", ThreadState::Running, b),
        ("c1", ThreadState::Gone, c),
        ("d1", ThreadState::Running, d),
    ]);
    let v = vm_with(&s, sync, NeedsList::default());
    assert_eq!(v.rows[&a].live, Some(ChipDto::WaitingOnYou));
    assert_eq!(v.rows[&b].live, Some(ChipDto::Working));
    assert_eq!(v.rows[&c].live, None, "a gone thread has no chip");
    assert_eq!(v.rows[&d].live, None, "only while the task is open");
}

#[test]
fn waits_on_lists_open_blockers_from_the_edges() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let open = add(&mut s, "Open", None);
    let done = add(&mut s, "Done", None);
    s.add_dependency(a, open, &Actor::Me).unwrap();
    s.add_dependency(a, done, &Actor::Me).unwrap();
    status(&mut s, done, Status::Done);
    let v = vm(&s);
    assert!(v.rows[&a].waiting);
    assert_eq!(v.rows[&a].waits_on, vec![open]);
    assert!(!v.rows[&a].ready);
    status(&mut s, open, Status::Done);
    let v = vm(&s);
    assert!(!v.rows[&a].waiting && v.rows[&a].waits_on.is_empty());
    assert!(v.rows[&a].ready && v.rows[&a].ready_since.is_some());
}

#[test]
fn needs_items_set_the_dot_the_ready_pill_quiet_days_and_staleness() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    let now = common::at(common::NOW);
    let six_days = airboss_services::time::to_ts(now - Duration::from_secs(6 * 86_400 + 60));
    let mut quiet = needs_item("quiet:a", NeedsKind::Quiet, a, &six_days);
    quiet.stale = true;
    let needs = NeedsList {
        count: 2,
        items: vec![
            quiet,
            needs_item("ready:b", NeedsKind::Ready, b, common::NOW),
        ],
        checked_at: None,
        version: 1,
    };
    let v = vm_with(&s, SyncSnapshot::default(), needs);
    assert_eq!(v.rows[&a].quiet_days, Some(6));
    assert!(v.rows[&a].needs && v.rows[&a].needs_stale && !v.rows[&a].ready_pill);
    assert!(v.rows[&b].ready_pill && v.rows[&b].needs && !v.rows[&b].needs_stale);
}

#[test]
fn diff_sends_upserts_and_removals_and_order_only_when_it_changed() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    let v1 = vm(&s);
    assert!(diff(&v1, &v1).is_none(), "nothing changed, no patch");

    s.edit_task(a, TaskEdit::default().title("A2"), &Actor::Me)
        .unwrap();
    let v2 = vm(&s);
    let p = diff(&v1, &v2).unwrap();
    assert_eq!(p.upserts.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a]);
    assert!(p.order.is_none() && p.repos.is_none() && p.removed.is_empty() && p.created.is_empty());

    let c = add(&mut s, "C", None);
    let v3 = vm(&s);
    let p = diff(&v2, &v3).unwrap();
    assert_eq!(p.order, Some(vec![a, b, c]));
    assert_eq!(p.created, vec![c]);

    s.delete_task(b, &Actor::Me).unwrap();
    let v4 = vm(&s);
    let p = diff(&v3, &v4).unwrap();
    assert_eq!(p.removed, vec![b]);
    assert_eq!(p.order, Some(vec![a, c]));

    s.add_repo("web", Path::new("/r/web"), None, &Actor::Me)
        .unwrap();
    let v5 = vm(&s);
    let p = diff(&v4, &v5).unwrap();
    assert_eq!(p.repos.unwrap().len(), 1);
}

#[test]
fn a_subtask_change_upserts_its_ancestors() {
    let mut s = store();
    let top = add(&mut s, "Top", None);
    let mid = add(&mut s, "Mid", Some(top));
    let leaf = add(&mut s, "Leaf", Some(mid));
    let other = add(&mut s, "Other", None);
    let v1 = vm(&s);
    status(&mut s, leaf, Status::Doing);
    let v2 = vm(&s);
    let ids: Vec<TaskId> = diff(&v1, &v2)
        .unwrap()
        .upserts
        .iter()
        .map(|r| r.id)
        .collect();
    assert!(ids.contains(&leaf) && ids.contains(&mid) && ids.contains(&top));
    assert!(!ids.contains(&other));
}

#[test]
fn seq_always_increases_across_writes_and_service_bumps() {
    let app = TestApp::new();
    let a = app.add("A", None);
    app.sync_vm();
    let mut last = app.state.seq();
    for i in 0..5 {
        app.core(|s| s.edit_task(a, TaskEdit::default().title(format!("A{i}")), &Actor::Me))
            .unwrap();
        let p = app.sync_vm().unwrap();
        assert!(p.seq > last);
        last = p.seq;
        // Alternate the dot on and off, so each bump changes the row.
        let items = if i % 2 == 0 {
            vec![needs_item(
                &format!("k{i}"),
                NeedsKind::Quiet,
                a,
                common::NOW,
            )]
        } else {
            Vec::new()
        };
        app.attention.set_items(items);
        let p = app.state.catch_up_services().unwrap();
        assert!(p.seq > last);
        last = p.seq;
    }
    let seqs: Vec<u64> = app.patches().iter().map(|p| p.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
}

#[test]
fn a_service_bump_rebuilds_without_reading_core() {
    let app = TestApp::new();
    let a = app.add("A", None);
    app.sync_vm();
    app.attention
        .set_items(vec![needs_item("q", NeedsKind::Question, a, common::NOW)]);
    // Hold the store lock: a rebuild that read core would block until the timeout.
    let guard = app.store.lock().unwrap();
    let state = app.state.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let t = Instant::now();
        let p = state.catch_up_services();
        tx.send((p, t.elapsed())).unwrap();
    });
    let (p, took) = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("rebuild read core");
    drop(guard);
    let p = p.unwrap();
    assert!(p.upserts.iter().any(|r| r.id == a && r.needs));
    assert!(took < Duration::from_millis(200));
}

// ---------------------------------------------------------------------------- plans

#[test]
fn plan_cards_show_progress_waves_and_whats_next() {
    let mut s = store();
    let p = add(&mut s, "Plan", None);
    let a = add(&mut s, "A", Some(p));
    let b = add(&mut s, "B", Some(p));
    let c = add(&mut s, "C", Some(p));
    s.add_dependency(b, a, &Actor::Me).unwrap();
    s.add_dependency(c, b, &Actor::Me).unwrap();
    status(&mut s, a, Status::Done);
    let _closed = {
        let q = add(&mut s, "Closed plan", None);
        let q1 = add(&mut s, "Q1", Some(q));
        status(&mut s, q1, Status::Done);
        status(&mut s, q, Status::Done);
        q
    };
    let _leaf = add(&mut s, "Just a task", None);

    let cards = plan_cards(&vm(&s));
    assert_eq!(cards.len(), 1, "only open top-level parents");
    let card = &cards[0];
    assert_eq!(
        (card.progress.done, card.progress.total, card.percent),
        (1, 3, 33)
    );
    assert_eq!(card.wave_count, 3);
    assert_eq!(card.current_wave, Some(1));
    assert_eq!(
        card.wave_dots,
        vec![WaveDot::Done, WaveDot::Current, WaveDot::Todo]
    );
    assert_eq!(card.next_up, NextUp::Ready { ids: vec![b] });

    status(&mut s, b, Status::Done);
    status(&mut s, c, Status::Done);
    let cards = plan_cards(&vm(&s));
    assert_eq!(cards[0].next_up, NextUp::FinishIt);
    assert_eq!(cards[0].current_wave, None);
}

// ---------------------------------------------------------------------------- run map

/// DESIGN's Refunds example (screen 03).
fn refunds(s: &mut Store) -> (TaskId, HashMap<&'static str, TaskId>) {
    let outside = add(s, "MCP server", None);
    let p = add(s, "Refunds", None);
    let mut ids = HashMap::new();
    for name in ["201", "202", "203", "204", "205", "206", "207", "208"] {
        ids.insert(name, add(s, name, Some(p)));
    }
    let dep = |s: &mut Store, a: TaskId, b: TaskId| s.add_dependency(a, b, &Actor::Me).unwrap();
    dep(s, ids["202"], ids["201"]);
    dep(s, ids["203"], ids["201"]);
    dep(s, ids["205"], ids["202"]);
    dep(s, ids["205"], ids["204"]);
    dep(s, ids["207"], ids["205"]);
    dep(s, ids["208"], outside);
    status(s, ids["201"], Status::Done);
    status(s, ids["202"], Status::Doing);
    status(s, ids["203"], Status::Blocked);
    status(s, ids["204"], Status::InReview);
    (p, ids)
}

#[test]
fn the_refunds_example_has_waves_of_3_2_1_and_2_with_one_outside_blocker() {
    let mut s = store();
    let (p, ids) = refunds(&mut s);
    let v = vm(&s);
    let w = display_waves(&v, p);
    let names = |wave: &Vec<TaskId>| {
        wave.iter()
            .map(|id| *ids.iter().find(|(_, v)| *v == id).unwrap().0)
            .collect::<Vec<_>>()
    };
    let waves: Vec<Vec<&str>> = w.waves.iter().map(names).collect();
    assert_eq!(
        waves,
        vec![
            vec!["201", "204", "206"],
            vec!["202", "203"],
            vec!["205"],
            vec!["207", "208"],
        ]
    );
    assert_eq!(w.outside.len(), 1);
    assert_eq!(w.outside[0].task, ids["208"]);
    // Done 201 is kept, and its lines are solid; the others are dashed.
    let edge = |from: &str, to: &str| {
        w.edges
            .iter()
            .find(|e| e.from == ids[from] && e.to == ids[to])
            .unwrap()
            .done
    };
    assert!(edge("201", "202") && edge("201", "203"));
    assert!(!edge("202", "205") && !edge("204", "205") && !edge("205", "207"));

    let order = s.run_order(p).unwrap();
    let map = run_map(&v, p, &order, 7);
    assert_eq!(map.req, 7);
    assert_eq!(map.nodes.len(), 8);
    assert_eq!(map.outside.len(), 1);
    assert!(map.fits);
}

#[test]
fn a_phase_step_waiting_on_another_phase_lifts_to_the_phases() {
    let mut s = store();
    let p = add(&mut s, "Project", None);
    let ph1 = add(&mut s, "Phase 1", Some(p));
    let ph2 = add(&mut s, "Phase 2", Some(p));
    let s1 = add(&mut s, "Step 1", Some(ph1));
    let s2 = add(&mut s, "Step 2", Some(ph2));
    s.add_dependency(s2, s1, &Actor::Me).unwrap();
    let w = display_waves(&vm(&s), p);
    assert_eq!(w.waves, vec![vec![ph1], vec![ph2]]);
    assert_eq!(w.edges.len(), 1);
    assert_eq!(
        (w.edges[0].from, w.edges[0].to, w.edges[0].done),
        (ph1, ph2, false)
    );
    status(&mut s, s1, Status::Done);
    let w = display_waves(&vm(&s), p);
    assert!(w.edges[0].done, "done once every blocker behind it is");
}

#[test]
fn a_tangled_pair_is_kept_apart_and_dropped_or_proposed_subtasks_are_left_out() {
    let mut s = store();
    let p = add(&mut s, "Project", None);
    let ph1 = add(&mut s, "Phase 1", Some(p));
    let ph2 = add(&mut s, "Phase 2", Some(p));
    let free = add(&mut s, "Free", Some(p));
    let gone = add(&mut s, "Gone", Some(p));
    s.add_task(
        NewTask::new("Proposed").under(p),
        &Actor::Agent("t3/x".into()),
    )
    .unwrap();
    let a1 = add(&mut s, "a1", Some(ph1));
    let a2 = add(&mut s, "a2", Some(ph1));
    let b1 = add(&mut s, "b1", Some(ph2));
    let b2 = add(&mut s, "b2", Some(ph2));
    s.add_dependency(b1, a1, &Actor::Me).unwrap();
    s.add_dependency(a2, b2, &Actor::Me).unwrap();
    status(&mut s, gone, Status::Dropped);
    let w = display_waves(&vm(&s), p);
    assert_eq!(w.waves, vec![vec![free]]);
    assert_eq!(w.tangled, vec![ph1, ph2]);
    assert_eq!(w.nodes, vec![ph1, ph2, free]);
}

#[test]
fn the_map_falls_back_above_40_subtasks_or_8_waves() {
    assert!(map_fits(40, 8));
    assert!(!map_fits(41, 1));
    assert!(!map_fits(10, 9));
    let mut s = store();
    let p = add(&mut s, "Chain", None);
    let mut prev = None;
    for i in 0..9 {
        let t = add(&mut s, &format!("step {i}"), Some(p));
        if let Some(b) = prev {
            s.add_dependency(t, b, &Actor::Me).unwrap();
        }
        prev = Some(t);
    }
    let v = vm(&s);
    let order = s.run_order(p).unwrap();
    assert!(!run_map(&v, p, &order, 0).fits);
}

#[test]
fn rows_follow_status_for_the_ui() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    status(&mut s, a, Status::Blocked);
    let v = vm(&s);
    assert_eq!(v.rows[&a].status, StatusDto::Blocked);
    assert_eq!(v.rows[&a].blocked_reason.as_deref(), Some("stuck"));
    let _ = SystemTime::now();
}
