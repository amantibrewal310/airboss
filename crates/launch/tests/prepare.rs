//! `prepare`: what Do it and Plan it would do, read-only, with no process run.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use airboss_core::{Actor, NewTask, Status, TaskId};
use airboss_services::{ErrorCode, LaunchService, Mode};
use common::{H, ids, t3_key};

#[test]
fn a_task_with_its_own_repos() {
    let h = H::new();
    let web = h.repo("booking-web");
    let api = h.repo("booking-api");
    h.repo("unrelated");
    let t = h.task("Booking flow", &[&web, &api]);
    let p = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert_eq!(
        p.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
        ids(&[&api, &web])
    );
    assert!(!p.from_subtasks);
    assert!(!p.needs_choice);
    assert_eq!(p.choices.len(), 3);
    assert_eq!(p.branch, Some(format!("task-{}-booking-flow", t.id.0)));
    assert_eq!(p.research_dir, h.cfg.research_dir);
    assert!(p.waiting_on.is_empty());
    assert!(p.started.is_empty());
    // Not saved.
    assert_eq!(h.get(t.id).branch, None);
}

#[test]
fn a_task_with_only_its_subtasks_repos() {
    let h = H::new();
    let web = h.repo("web");
    let api = h.repo("api");
    let old = h.repo("old");
    let parent = h.task("Parent", &[]);
    let phase = {
        let mut s = h.store.lock().unwrap();
        s.add_task(NewTask::new("Phase").under(parent.id), &Actor::Me)
            .unwrap()
    };
    let mut s = h.store.lock().unwrap();
    s.add_task(
        NewTask::new("Step a").under(phase.id).in_repo(web.id),
        &Actor::Me,
    )
    .unwrap();
    s.add_task(
        NewTask::new("Step b")
            .under(parent.id)
            .in_repo(api.id)
            .in_repo(web.id),
        &Actor::Me,
    )
    .unwrap();
    let done = s
        .add_task(
            NewTask::new("Done step").under(parent.id).in_repo(old.id),
            &Actor::Me,
        )
        .unwrap();
    s.set_status(done.id, Status::Done, None, &Actor::Me)
        .unwrap();
    drop(s);
    let p = h.launch.prepare(parent.id, Mode::DoIt).unwrap();
    assert_eq!(
        p.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
        ids(&[&api, &web])
    );
    assert!(p.from_subtasks);
    assert!(!p.needs_choice);
}

#[test]
fn a_task_with_no_repo() {
    let h = H::new();
    h.repo("web");
    let t = h.task("Research hotels", &[]);
    let p = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert!(p.repos.is_empty());
    assert!(p.needs_choice);
    assert_eq!(p.choices.len(), 1);
    assert_eq!(p.branch, None);
    // Plan it doesn't ask: it plans in the research folder.
    let p = h.launch.prepare(t.id, Mode::PlanIt).unwrap();
    assert!(!p.needs_choice);
    assert_eq!(p.branch, None);
}

#[test]
fn the_tasks_own_branch_is_kept() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("Booking flow", &[&web]);
    h.store
        .lock()
        .unwrap()
        .set_branch(t.id, Some("feature/booking"), &Actor::Me)
        .unwrap();
    let p = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert_eq!(p.branch.as_deref(), Some("feature/booking"));
}

#[test]
fn open_blockers_are_listed() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("Waits", &[&web]);
    let a = h.task("A", &[]);
    let b = h.task("B", &[]);
    let c = h.task("C", &[]);
    let mut s = h.store.lock().unwrap();
    for blocker in [a.id, b.id, c.id] {
        s.add_dependency(t.id, blocker, &Actor::Me).unwrap();
    }
    s.set_status(c.id, Status::Done, None, &Actor::Me).unwrap();
    drop(s);
    let p = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert_eq!(p.waiting_on, vec![a.id, b.id]);
}

#[test]
fn proposed_done_and_dropped_tasks_are_refused() {
    let h = H::new();
    let mut s = h.store.lock().unwrap();
    let proposed = s
        .add_task(NewTask::new("Idea"), &Actor::Agent("t3/x".into()))
        .unwrap();
    let done = s.add_task(NewTask::new("Done"), &Actor::Me).unwrap();
    s.set_status(done.id, Status::Done, None, &Actor::Me)
        .unwrap();
    let dropped = s.add_task(NewTask::new("Dropped"), &Actor::Me).unwrap();
    s.set_status(dropped.id, Status::Dropped, None, &Actor::Me)
        .unwrap();
    drop(s);
    for mode in [Mode::DoIt, Mode::PlanIt] {
        let e = h.launch.prepare(proposed.id, mode).unwrap_err();
        assert_eq!(e.code, ErrorCode::TaskProposed);
        let e = h.launch.prepare(done.id, mode).unwrap_err();
        assert_eq!(e.code, ErrorCode::TaskClosed);
        assert_eq!(
            e.message,
            format!("Task {} is done. Reopen it to start it.", done.id.0)
        );
        let e = h.launch.prepare(dropped.id, mode).unwrap_err();
        assert_eq!(e.code, ErrorCode::TaskClosed);
    }
    let e = h.launch.prepare(TaskId(999), Mode::DoIt).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
}

#[test]
fn prepare_runs_no_process() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("Booking flow", &[&web]);
    h.launch.prepare(t.id, Mode::DoIt).unwrap();
    h.launch.prepare(t.id, Mode::PlanIt).unwrap();
    assert!(h.t3.calls().is_empty());
    assert!(h.git.calls().is_empty());
}

#[test]
fn started_threads_are_listed() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("Booking flow", &[&web]);
    h.db()
        .execute(
            "INSERT INTO launch_threads (task_id, repo_id, mode, state, thread_id, project_path,
               branch, base_ref, title, linked, created_at)
             VALUES (?1, ?2, 'do', 'started', 'thread-9', '/x', 'b', 'main', '1 T', 1,
               '2026-11-26T10:00:00.000Z'),
                    (?1, NULL, 'plan', 'pending', NULL, '/x', NULL, NULL, 'Plan 1 T', 0,
               '2026-11-26T10:00:00.000Z')",
            [t.id.0, web.id.0],
        )
        .unwrap();
    let p = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert_eq!(p.started.len(), 1);
    let s = &p.started[0];
    assert_eq!(s.key, t3_key("thread-9"));
    assert_eq!((s.mode, s.repo, s.linked), (Mode::DoIt, Some(web.id), true));
    assert_eq!(s.title, "1 T");
}
