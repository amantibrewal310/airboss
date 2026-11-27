//! Parents are containers (rule 10): a task with steps shows a status worked out from them,
//! and takes no status change but a drop, no branch and no PR of its own. Also rule 12, which
//! lets a parent take new steps whatever it shows.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, NewLink, NewTask, PrState, Status, Store, TaskId};
use common::{agent, store, tick};

fn top(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn under(s: &mut Store, parent: TaskId, title: &str) -> TaskId {
    s.add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
}

fn set(s: &mut Store, id: TaskId, to: Status) {
    let note = (to == Status::Blocked).then_some("waiting on keys");
    s.set_status(id, to, note, &Actor::Me).unwrap();
}

fn status(s: &Store, id: TaskId) -> Status {
    s.task(id).unwrap().status
}

fn tree_status(s: &Store, id: TaskId) -> Status {
    let tree = s.tree().unwrap();
    tree.iter().find(|n| n.task.id == id).unwrap().task.status
}

/// A plan with two steps, read the same way by `task()` and `tree()`.
fn shown_with(steps: [Status; 2]) -> Status {
    let mut s = store();
    let p = top(&mut s, "Plan");
    for (i, to) in steps.into_iter().enumerate() {
        let step = under(&mut s, p, &format!("Step {i}"));
        if to != Status::Todo {
            set(&mut s, step, to);
        }
    }
    let shown = status(&s, p);
    assert_eq!(tree_status(&s, p), shown, "task() and tree() agree");
    shown
}

#[test]
fn a_parent_shows_what_its_steps_add_up_to() {
    use Status::*;
    assert_eq!(shown_with([Done, Done]), Done);
    assert_eq!(shown_with([Done, Dropped]), Done);
    assert_eq!(shown_with([Dropped, Dropped]), Dropped);
    assert_eq!(shown_with([Todo, Todo]), Todo);
    assert_eq!(shown_with([Todo, Doing]), Doing);
    assert_eq!(shown_with([Todo, InReview]), Doing);
    assert_eq!(
        shown_with([Todo, Done]),
        Doing,
        "partly done is in progress"
    );
    assert_eq!(shown_with([Doing, Blocked]), Blocked);
}

#[test]
fn a_blocked_parent_says_which_step_is_blocked() {
    let mut s = store();
    let plan = top(&mut s, "Plan");
    let phase = under(&mut s, plan, "Phase");
    let step = under(&mut s, phase, "Step");
    set(&mut s, step, Status::Blocked);
    let why = format!("#{} is blocked: waiting on keys", step.0);
    for id in [plan, phase] {
        let t = s.task(id).unwrap();
        assert_eq!(
            (t.status, t.blocked_reason.as_deref()),
            (Status::Blocked, Some(&*why))
        );
    }
}

#[test]
fn a_parents_stored_status_is_ignored() {
    let mut s = store();
    let p = top(&mut s, "Was in review");
    set(&mut s, p, Status::InReview);
    let step = under(&mut s, p, "New step");
    assert_eq!(status(&s, p), Status::Todo);
    set(&mut s, step, Status::Done);
    assert_eq!(status(&s, p), Status::Done);
    let tree = s.tree().unwrap();
    let node = tree.iter().find(|n| n.task.id == p).unwrap();
    assert!(node.has_steps);
    assert!(s.has_steps(p).unwrap() && !s.has_steps(step).unwrap());
}

#[test]
fn proposals_are_not_steps() {
    let mut s = store();
    let p = top(&mut s, "Mine");
    set(&mut s, p, Status::Doing);
    let idea = s
        .add_task(NewTask::new("Idea").under(p), &agent())
        .unwrap()
        .id;
    assert_eq!(status(&s, p), Status::Doing, "its own status, still");
    assert!(!s.has_steps(p).unwrap());
    s.reject_proposals(&[idea], None, &Actor::Me).unwrap();
    assert!(!s.has_steps(p).unwrap(), "a rejected proposal isn't a step");

    let accepted = s
        .add_task(NewTask::new("Accepted").under(p), &agent())
        .unwrap()
        .id;
    set(&mut s, accepted, Status::Todo);
    assert!(s.has_steps(p).unwrap());
    assert_eq!(status(&s, p), Status::Todo);
}

#[test]
fn nothing_but_a_drop_changes_a_parents_status() {
    let mut s = store();
    let p = top(&mut s, "Plan");
    let a = under(&mut s, p, "A");
    let b = under(&mut s, p, "B");
    for to in [Status::Done, Status::Doing, Status::Blocked, Status::Todo] {
        let err = s.set_status(p, to, Some("why"), &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::HasSteps), "{to}: {err:?}");
    }
    for actor in [agent(), Actor::SyncGh, Actor::Launch] {
        let err = s
            .set_status(p, Status::Done, Some("did it"), &actor)
            .unwrap_err();
        assert!(matches!(err, Error::HasSteps), "{actor}: {err:?}");
        let err = s.set_status(p, Status::Dropped, None, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }

    // Dropping it drops the open steps; one already done makes it done.
    set(&mut s, a, Status::Done);
    let events = s.history(p).unwrap().len();
    let got = s.set_status(p, Status::Dropped, None, &Actor::Me).unwrap();
    assert_eq!(got.status, Status::Done);
    assert_eq!(status(&s, b), Status::Dropped);
    assert_eq!(
        s.history(p).unwrap().len(),
        events,
        "no event on the parent"
    );
}

#[test]
fn a_parent_takes_no_branch_but_can_lose_an_old_one() {
    let mut s = store();
    let p = top(&mut s, "Plan");
    s.set_branch(p, Some("old-branch"), &Actor::Launch).unwrap();
    under(&mut s, p, "Step");
    let err = s.set_branch(p, Some("new"), &Actor::Launch).unwrap_err();
    assert!(matches!(err, Error::HasSteps), "{err:?}");
    assert_eq!(s.task(p).unwrap().branch.as_deref(), Some("old-branch"));
    s.set_branch(p, None, &Actor::Me).unwrap();
    assert_eq!(s.task(p).unwrap().branch, None);
}

#[test]
fn a_parent_takes_no_pr_and_its_old_ones_reopen_nothing() {
    let mut s = store();
    let p = top(&mut s, "Plan");
    s.link(p, NewLink::pr("acme/web#54"), &agent()).unwrap();
    let step = under(&mut s, p, "Step");
    for actor in [Actor::Me, agent(), Actor::SyncGh] {
        let err = s.link(p, NewLink::pr("acme/web#55"), &actor).unwrap_err();
        assert!(matches!(err, Error::HasSteps), "{actor}: {err:?}");
    }
    // Other links still go on a parent, and the old PR stays as history.
    s.link(
        p,
        NewLink::new(airboss_core::LinkKind::Doc, "plan.md"),
        &agent(),
    )
    .unwrap();
    assert_eq!(s.links(p).unwrap().len(), 2);

    set(&mut s, step, Status::Done);
    s.set_pr_state("acme/web#54", PrState::Merged, &Actor::SyncGh)
        .unwrap();
    s.set_pr_state("acme/web#54", PrState::Open, &Actor::SyncGh)
        .unwrap();
    assert_eq!(status(&s, p), Status::Done, "follows its step, not its PR");
    let last = s.history(p).unwrap().pop().unwrap();
    assert_eq!(last.kind, EventKind::LinkState);
}

#[test]
fn waiting_on_a_parent_ends_when_its_steps_finish_and_ready_dates_from_the_last() {
    let mut s = store();
    let phase1 = top(&mut s, "Phase 1");
    let a = under(&mut s, phase1, "A");
    let b = under(&mut s, phase1, "B");
    let next = top(&mut s, "Next");
    s.add_dependency(next, phase1, &Actor::Me).unwrap();
    let waiting = |s: &Store| {
        let tree = s.tree().unwrap();
        tree.iter().find(|n| n.task.id == next).unwrap().waiting
    };
    assert!(waiting(&s));
    set(&mut s, a, Status::Done);
    assert!(waiting(&s));
    assert_eq!(s.waits_on(next).unwrap().len(), 1);
    tick();
    set(&mut s, b, Status::Done);
    assert!(!waiting(&s));
    assert!(s.waits_on(next).unwrap().is_empty());
    let deps = s.dependencies(next).unwrap();
    assert_eq!(deps.waits_on[0].status, Status::Done);

    let ready = s.ready().unwrap();
    let r = ready.iter().find(|r| r.task.id == next).unwrap();
    let finished = s.history(b).unwrap().pop().unwrap().at;
    assert_eq!(r.since, finished);
    assert!(
        ready.iter().all(|r| r.task.id != phase1),
        "a parent is never ready itself"
    );
}

#[test]
fn run_order_reads_each_phase_as_it_shows() {
    let mut s = store();
    let plan = top(&mut s, "Plan");
    let p1 = under(&mut s, plan, "Phase 1");
    let p1a = under(&mut s, p1, "Step");
    let p2 = under(&mut s, plan, "Phase 2");
    under(&mut s, p2, "Step");
    s.add_dependency(p2, p1, &Actor::Me).unwrap();
    let waves = |s: &Store| -> Vec<Vec<TaskId>> {
        let order = s.run_order(plan).unwrap();
        order
            .waves
            .iter()
            .map(|w| w.iter().map(|t| t.id).collect())
            .collect()
    };
    assert_eq!(waves(&s), vec![vec![p1], vec![p2]]);
    set(&mut s, p1a, Status::Done);
    // Phase 1 shows done, so it leaves the order and Phase 2 can run.
    assert_eq!(waves(&s), vec![vec![p2]]);
}

#[test]
fn a_parent_takes_new_steps_whatever_it_shows_but_a_finished_task_does_not() {
    let mut s = store();
    let p = top(&mut s, "Plan");
    let step = under(&mut s, p, "Step");
    set(&mut s, step, Status::Done);
    assert_eq!(status(&s, p), Status::Done);
    let more = under(&mut s, p, "One more");
    assert_eq!(status(&s, p), Status::Doing);

    let leaf = top(&mut s, "Finished");
    set(&mut s, leaf, Status::Done);
    let err = s
        .add_task(NewTask::new("Under it").under(leaf), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    let err = s.move_task(more, Some(leaf), 1, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
}
