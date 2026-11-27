//! Dependencies, loops, waiting and run order (rules 14–18).
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventKind, NewTask, RunOrder, Status, Store, Task, TaskId};
use common::{store, tick};

fn top(store: &mut Store, title: &str) -> TaskId {
    store.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn under(store: &mut Store, parent: TaskId, title: &str) -> TaskId {
    store
        .add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
}

fn ready_tasks(s: &Store) -> Vec<Task> {
    s.ready().unwrap().into_iter().map(|r| r.task).collect()
}

fn ids(tasks: &[Task]) -> Vec<TaskId> {
    tasks.iter().map(|t| t.id).collect()
}

fn waiting(store: &Store, id: TaskId) -> bool {
    store
        .tree()
        .unwrap()
        .into_iter()
        .find(|n| n.task.id == id)
        .unwrap()
        .waiting
}

fn kinds(store: &Store, id: TaskId) -> Vec<EventKind> {
    store
        .history(id)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

// ---- T10: dependencies, loops and waiting ----

// Rule 27: every dependency, both ways, finished ones included.

#[test]
fn dependencies_lists_both_directions_with_finished_blockers() {
    let mut s = store();
    let [a, b, c, d, e] = ["A", "B", "C", "D", "E"].map(|t| top(&mut s, t));
    // C waits on A (done) and B (open); D and E wait on C.
    for (task, on) in [(c, b), (c, a), (d, c), (e, c)] {
        s.add_dependency(task, on, &Actor::Me).unwrap();
    }
    set_status(&mut s, a, Status::Done);
    set_status(&mut s, e, Status::Dropped);

    let deps = s.dependencies(c).unwrap();
    assert_eq!(ids(&deps.waits_on), vec![a, b]);
    assert_eq!(ids(&deps.blocks), vec![d, e]);
    assert_eq!(deps.waits_on[0].status, Status::Done);
    // `waits_on` still lists open blockers only.
    assert_eq!(ids(&s.waits_on(c).unwrap()), vec![b]);
}

#[test]
fn a_task_with_no_dependencies_has_empty_lists() {
    let mut s = store();
    let a = top(&mut s, "A");
    let deps = s.dependencies(a).unwrap();
    assert!(deps.waits_on.is_empty() && deps.blocks.is_empty());
}

#[test]
fn dependencies_of_a_missing_task_is_not_found() {
    let s = store();
    assert!(matches!(
        s.dependencies(TaskId(8)).unwrap_err(),
        Error::NotFound
    ));
}

#[test]
fn a_chain_waits_link_by_link_and_only_its_head_is_ready() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    let c = top(&mut s, "C");
    s.add_dependency(b, a, &Actor::Me).unwrap();
    s.add_dependency(c, b, &Actor::Me).unwrap();

    assert_eq!(ids(&s.waits_on(c).unwrap()), vec![b]);
    assert_eq!(ids(&s.waits_on(b).unwrap()), vec![a]);
    assert!(s.waits_on(a).unwrap().is_empty());
    assert_eq!(
        (waiting(&s, a), waiting(&s, b), waiting(&s, c)),
        (false, true, true)
    );
    assert_eq!(ids(&ready_tasks(&s)), vec![a]);
}

/// Builds `n` tasks, each under its own parent, chained so task i waits on task i+1, then
/// tries to close the loop.
fn loop_across_parents(n: usize) {
    let mut s = store();
    let tasks: Vec<TaskId> = (0..n)
        .map(|i| {
            let p = top(&mut s, &format!("Parent {i}"));
            under(&mut s, p, &format!("Step {i}"))
        })
        .collect();
    for pair in tasks.windows(2) {
        s.add_dependency(pair[0], pair[1], &Actor::Me).unwrap();
    }
    let last = tasks[n - 1];
    let before = s.history(last).unwrap().len();
    assert!(matches!(
        s.add_dependency(last, tasks[0], &Actor::Me),
        Err(Error::Loop)
    ));
    assert!(s.waits_on(last).unwrap().is_empty(), "nothing written");
    assert_eq!(s.history(last).unwrap().len(), before, "no event written");
}

#[test]
fn a_loop_of_two_is_rejected() {
    loop_across_parents(2);
}

#[test]
fn a_loop_of_three_across_parents_is_rejected() {
    loop_across_parents(3);
}

#[test]
fn a_loop_of_five_across_parents_is_rejected() {
    loop_across_parents(5);
}

#[test]
fn a_task_cannot_wait_on_itself() {
    let mut s = store();
    let a = top(&mut s, "A");
    assert!(matches!(
        s.add_dependency(a, a, &Actor::Me),
        Err(Error::Loop)
    ));
}

#[test]
fn a_task_cannot_wait_on_its_ancestor_or_descendant() {
    let mut s = store();
    let parent = top(&mut s, "Parent");
    let child = under(&mut s, parent, "Child");
    let grandchild = under(&mut s, child, "Grandchild");
    for (task, waits_on) in [
        (parent, child),
        (child, parent),
        (parent, grandchild),
        (grandchild, parent),
    ] {
        assert!(
            matches!(
                s.add_dependency(task, waits_on, &Actor::Me),
                Err(Error::InvalidTarget)
            ),
            "{task:?} waiting on {waits_on:?}"
        );
    }
    assert!(s.waits_on(parent).unwrap().is_empty());
    assert!(s.waits_on(grandchild).unwrap().is_empty());
}

#[test]
fn cousins_may_wait_on_each_other() {
    let mut s = store();
    let p = top(&mut s, "Parent");
    let a = under(&mut s, p, "A");
    let b = under(&mut s, p, "B");
    let b1 = under(&mut s, b, "B1");
    s.add_dependency(b1, a, &Actor::Me).unwrap();
    assert_eq!(ids(&s.waits_on(b1).unwrap()), vec![a]);
}

#[test]
fn adding_writes_one_event_on_the_waiting_task_only() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    s.add_dependency(b, a, &Actor::Plan).unwrap();

    let history = s.history(b).unwrap();
    let last = history.last().unwrap();
    assert_eq!(
        (last.kind, &last.actor),
        (EventKind::WaitsOnAdded, &Actor::Plan)
    );
    assert_eq!(
        kinds(&s, b),
        vec![EventKind::Created, EventKind::WaitsOnAdded]
    );
    assert_eq!(kinds(&s, a), vec![EventKind::Created]);
}

#[test]
fn adding_an_existing_dependency_changes_nothing() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    s.add_dependency(b, a, &Actor::Me).unwrap();
    s.add_dependency(b, a, &Actor::Me).unwrap();
    assert_eq!(
        kinds(&s, b),
        vec![EventKind::Created, EventKind::WaitsOnAdded]
    );
    assert_eq!(ids(&s.waits_on(b).unwrap()), vec![a]);
}

#[test]
fn removing_a_dependency_stops_the_wait_and_writes_an_event() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    s.add_dependency(b, a, &Actor::Me).unwrap();
    s.remove_dependency(b, a, &Actor::Plan).unwrap();

    assert!(s.waits_on(b).unwrap().is_empty());
    assert!(!waiting(&s, b));
    assert_eq!(ids(&ready_tasks(&s)), vec![a, b]);
    assert_eq!(
        kinds(&s, b),
        vec![
            EventKind::Created,
            EventKind::WaitsOnAdded,
            EventKind::WaitsOnRemoved
        ]
    );
    assert_eq!(kinds(&s, a), vec![EventKind::Created]);
}

#[test]
fn removing_a_dependency_that_does_not_exist_is_not_found() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    assert!(matches!(
        s.remove_dependency(b, a, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert_eq!(kinds(&s, b), vec![EventKind::Created]);
}

#[test]
fn dependencies_on_missing_tasks_are_not_found() {
    let mut s = store();
    let a = top(&mut s, "A");
    let ghost = TaskId(999);
    assert!(matches!(
        s.add_dependency(a, ghost, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.add_dependency(ghost, a, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.remove_dependency(ghost, a, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert!(matches!(s.waits_on(ghost), Err(Error::NotFound)));
}

#[test]
fn only_me_and_plan_may_change_dependencies() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    let c = top(&mut s, "C");
    s.add_dependency(c, a, &Actor::Me).unwrap();
    for actor in [
        Actor::Agent("t3/a1".into()),
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        assert!(
            matches!(s.add_dependency(b, a, &actor), Err(Error::NotAllowed)),
            "add by {actor}"
        );
        assert!(
            matches!(s.remove_dependency(c, a, &actor), Err(Error::NotAllowed)),
            "remove by {actor}"
        );
    }
    assert!(s.waits_on(b).unwrap().is_empty());
    assert_eq!(ids(&s.waits_on(c).unwrap()), vec![a]);
    s.add_dependency(b, a, &Actor::Plan).unwrap();
    s.remove_dependency(c, a, &Actor::Plan).unwrap();
}

#[test]
fn ready_lists_unblocked_todo_tasks_in_tree_order() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let p1 = under(&mut s, p, "Step 1");
    let p2 = under(&mut s, p, "Step 2");
    let q = top(&mut s, "Other");
    let proposal = s
        .add_task(NewTask::new("Idea"), &Actor::Agent("t3/a1".into()))
        .unwrap()
        .id;
    s.add_dependency(p2, p1, &Actor::Me).unwrap();

    let ready = ids(&ready_tasks(&s));
    // The project has steps, so it isn't started itself; its steps are (rule 10).
    assert_eq!(ready, vec![p1, q]);
    assert!(!ready.contains(&proposal), "proposals are not ready");
}

fn set_status(store: &mut Store, id: TaskId, to: Status) {
    store.set_status(id, to, None, &Actor::Me).unwrap();
}

#[test]
fn a_waiting_task_becomes_ready_when_its_blocker_finishes() {
    for finished in [Status::Done, Status::Dropped] {
        let mut s = store();
        let a = top(&mut s, "A");
        let b = top(&mut s, "B");
        let c = top(&mut s, "C");
        s.add_dependency(c, a, &Actor::Me).unwrap();
        s.add_dependency(c, b, &Actor::Me).unwrap();
        assert!(waiting(&s, c));
        assert_eq!(ids(&ready_tasks(&s)), vec![a, b]);

        set_status(&mut s, a, finished);
        assert!(waiting(&s, c), "still waits on B");
        assert_eq!(ids(&s.waits_on(c).unwrap()), vec![b]);

        set_status(&mut s, b, finished);
        assert!(!waiting(&s, c));
        assert!(s.waits_on(c).unwrap().is_empty());
        assert_eq!(ids(&ready_tasks(&s)), vec![c], "{finished:?}");
    }
}

#[test]
fn ready_leaves_out_tasks_that_are_not_todo() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    set_status(&mut s, a, Status::Doing);
    assert_eq!(ids(&ready_tasks(&s)), vec![b]);
}

#[test]
fn a_loop_through_finished_tasks_is_still_a_loop() {
    let mut s = store();
    let a = top(&mut s, "A");
    let b = top(&mut s, "B");
    s.add_dependency(b, a, &Actor::Me).unwrap();
    set_status(&mut s, b, Status::Done);
    assert!(matches!(
        s.add_dependency(a, b, &Actor::Me),
        Err(Error::Loop)
    ));
}

// ---- T11: run order ----

fn wave_ids(order: &RunOrder) -> Vec<Vec<TaskId>> {
    order.waves.iter().map(|w| ids(w)).collect()
}

#[test]
fn a_diamond_runs_in_three_waves() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|t| under(&mut s, p, t));
    for (task, on) in [(b, a), (c, a), (d, b), (d, c)] {
        s.add_dependency(task, on, &Actor::Me).unwrap();
    }
    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![a], vec![b, c], vec![d]]);
    assert!(order.waiting_outside.is_empty());
}

#[test]
fn tasks_keep_their_sibling_order_within_a_wave() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let [z, y, x] = ["Z", "Y", "X"].map(|t| under(&mut s, p, t));
    s.add_dependency(z, x, &Actor::Me).unwrap();
    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![y, x], vec![z]]);
}

#[test]
fn steps_waiting_across_phases_make_the_phases_wait() {
    let mut s = store();
    let project = top(&mut s, "Project");
    let [p1, p2, p3] = ["Phase 1", "Phase 2", "Phase 3"].map(|t| under(&mut s, project, t));
    let s1a = under(&mut s, p1, "Step 1a");
    let s1b = under(&mut s, p1, "Step 1b");
    let s2a = under(&mut s, p2, "Step 2a");
    let s2a1 = under(&mut s, s2a, "Step 2a.1");
    let s2b = under(&mut s, p2, "Step 2b");
    under(&mut s, p3, "Step 3a");
    s.add_dependency(s2b, s1a, &Actor::Me).unwrap();
    s.add_dependency(s2a1, s1b, &Actor::Me).unwrap();
    // A dependency inside one phase doesn't make it wait on itself.
    s.add_dependency(s1b, s1a, &Actor::Me).unwrap();

    let order = s.run_order(project).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![p1, p3], vec![p2]]);

    // Inside a phase, its own steps are ordered.
    let inner = s.run_order(p1).unwrap();
    assert_eq!(wave_ids(&inner), vec![vec![s1a], vec![s1b]]);
}

#[test]
fn a_subtask_with_an_open_blocker_outside_waits_outside() {
    let mut s = store();
    let elsewhere = top(&mut s, "Elsewhere");
    let p = top(&mut s, "Project");
    let [a, b, c] = ["A", "B", "C"].map(|t| under(&mut s, p, t));
    let a1 = under(&mut s, a, "A1");
    s.add_dependency(a1, elsewhere, &Actor::Me).unwrap();
    // C waits on A, so it can't start before A either.
    s.add_dependency(c, a, &Actor::Me).unwrap();

    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![b]]);
    assert_eq!(
        outside(&order),
        vec![(a, vec![elsewhere]), (c, vec![elsewhere])]
    );
}

/// Each `waiting_outside` entry with its blockers.
fn outside(order: &RunOrder) -> Vec<(TaskId, Vec<TaskId>)> {
    order
        .waiting_outside
        .iter()
        .map(|w| (w.task.id, w.blockers.clone()))
        .collect()
}

#[test]
fn outside_blockers_are_named_directly_through_steps_and_through_siblings() {
    let mut s = store();
    let [x, y, z, done] = ["X", "Y", "Z", "Done"].map(|t| top(&mut s, t));
    let p = top(&mut s, "Project");
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|t| under(&mut s, p, t));
    let b1 = under(&mut s, b, "B1");
    let b2 = under(&mut s, b, "B2");
    // A waits on Z and X directly; B through its steps on Y and X; a finished blocker
    // holds nothing. C waits on A and B, so on all three; D is free.
    for (task, on) in [(a, z), (a, x), (b1, y), (b2, x), (b2, done), (c, a), (c, b)] {
        s.add_dependency(task, on, &Actor::Me).unwrap();
    }
    set_status(&mut s, done, Status::Done);

    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![d]]);
    assert_eq!(
        outside(&order),
        vec![(a, vec![x, z]), (b, vec![x, y]), (c, vec![x, y, z])]
    );
}

#[test]
fn a_loop_made_only_by_lifting_goes_in_a_tangled_group_after_the_waves() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let w = under(&mut s, p, "W");
    let x = under(&mut s, p, "X");
    let y = under(&mut s, p, "Y");
    let z = under(&mut s, p, "Z");
    let [x1, x2] = ["X1", "X2"].map(|t| under(&mut s, x, t));
    let [y1, y2] = ["Y1", "Y2"].map(|t| under(&mut s, y, t));
    // No loop between tasks, but X waits on Y and Y waits on X.
    s.add_dependency(x1, y1, &Actor::Me).unwrap();
    s.add_dependency(y2, x2, &Actor::Me).unwrap();
    // W waits on the tangle, so it can't be placed either; Z is free.
    s.add_dependency(w, x2, &Actor::Me).unwrap();

    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![z]]);
    assert_eq!(ids(&order.tangled), vec![w, x, y]);
    assert!(order.waiting_outside.is_empty());
}

#[test]
fn run_order_leaves_out_finished_subtasks_and_what_they_held_up() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|t| under(&mut s, p, t));
    s.add_dependency(c, a, &Actor::Me).unwrap();
    s.add_dependency(d, b, &Actor::Me).unwrap();
    set_status(&mut s, a, Status::Done);
    set_status(&mut s, b, Status::Dropped);

    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![c, d]]);
    assert!(order.waiting_outside.is_empty());
}

#[test]
fn a_finished_blocker_outside_does_not_hold_a_subtask() {
    let mut s = store();
    let elsewhere = top(&mut s, "Elsewhere");
    let p = top(&mut s, "Project");
    let a = under(&mut s, p, "A");
    s.add_dependency(a, elsewhere, &Actor::Me).unwrap();
    set_status(&mut s, elsewhere, Status::Done);

    let order = s.run_order(p).unwrap();
    assert_eq!(wave_ids(&order), vec![vec![a]]);
    assert!(order.waiting_outside.is_empty());
}

#[test]
fn a_finished_step_no_longer_orders_its_phase() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let p1 = under(&mut s, p, "Phase 1");
    let p2 = under(&mut s, p, "Phase 2");
    let s1 = under(&mut s, p1, "Step 1");
    under(&mut s, p1, "Step 1b");
    let s2 = under(&mut s, p2, "Step 2");
    s.add_dependency(s2, s1, &Actor::Me).unwrap();
    assert_eq!(wave_ids(&s.run_order(p).unwrap()), vec![vec![p1], vec![p2]]);

    set_status(&mut s, s1, Status::Done);
    assert_eq!(wave_ids(&s.run_order(p).unwrap()), vec![vec![p1, p2]]);
}

#[test]
fn run_order_of_a_missing_parent_is_not_found() {
    let s = store();
    assert!(matches!(s.run_order(TaskId(7)), Err(Error::NotFound)));
}

#[test]
fn a_task_with_no_subtasks_has_an_empty_run_order() {
    let mut s = store();
    let p = top(&mut s, "Leaf");
    assert_eq!(s.run_order(p).unwrap(), RunOrder::default());
}

#[test]
fn run_order_leaves_out_proposals_until_they_are_accepted() {
    let mut s = Store::open_in_memory().unwrap();
    let phase = s
        .add_task(NewTask::new("Booking phase"), &Actor::Me)
        .unwrap();
    let api = s
        .add_task(NewTask::new("API").under(phase.id), &Actor::Me)
        .unwrap();
    let idea = s
        .add_task(
            NewTask::new("Add retry").under(phase.id),
            &Actor::Agent("t3/a".into()),
        )
        .unwrap();
    let page = s
        .add_task(NewTask::new("Page").under(phase.id), &Actor::Me)
        .unwrap();
    // Even a dependency on the proposal doesn't hold the page back while it's unaccepted.
    s.add_dependency(page.id, idea.id, &Actor::Me).unwrap();

    let order = s.run_order(phase.id).unwrap();
    let waves: Vec<Vec<_>> = order
        .waves
        .iter()
        .map(|w| w.iter().map(|t| t.id).collect())
        .collect();
    assert_eq!(waves, vec![vec![api.id, page.id]]);

    s.set_status(idea.id, Status::Todo, None, &Actor::Me)
        .unwrap();
    let order = s.run_order(phase.id).unwrap();
    let waves: Vec<Vec<_>> = order
        .waves
        .iter()
        .map(|w| w.iter().map(|t| t.id).collect())
        .collect();
    assert_eq!(waves, vec![vec![api.id, idea.id], vec![page.id]]);
}

// Rule 26: a ready task knows since when.

fn since(s: &Store, id: TaskId) -> String {
    s.ready()
        .unwrap()
        .into_iter()
        .find(|r| r.task.id == id)
        .unwrap()
        .since
}

fn newest_at(s: &Store, id: TaskId) -> String {
    s.history(id).unwrap().pop().unwrap().at
}

#[test]
fn a_task_that_never_waited_is_ready_since_it_was_created() {
    let mut s = store();
    let a = top(&mut s, "A");
    tick();
    s.add_note(a, "later", &Actor::Me).unwrap();
    assert_eq!(since(&s, a), s.history(a).unwrap()[0].at);
}

#[test]
fn a_task_is_ready_since_its_last_blocker_finished() {
    let mut s = store();
    let [a, b, c] = ["A", "B", "C"].map(|t| top(&mut s, t));
    s.add_dependency(c, a, &Actor::Me).unwrap();
    s.add_dependency(c, b, &Actor::Me).unwrap();
    set_status(&mut s, b, Status::Dropped);
    assert!(!ids(&ready_tasks(&s)).contains(&c), "still waits on A");
    tick();
    set_status(&mut s, a, Status::Done);
    assert_eq!(since(&s, c), newest_at(&s, a));

    // Later events on the blockers or the task don't move it.
    tick();
    s.add_note(b, "after the fact", &Actor::Me).unwrap();
    s.add_note(c, "noted", &Actor::Me).unwrap();
    assert_eq!(since(&s, c), s.history(a).unwrap().pop().unwrap().at);
}

#[test]
fn a_rejected_blocker_frees_its_waiter_at_the_rejection() {
    let mut s = store();
    let idea = s
        .add_task(NewTask::new("Idea"), &Actor::Agent("t3/a1".into()))
        .unwrap()
        .id;
    let c = top(&mut s, "C");
    s.add_dependency(c, idea, &Actor::Me).unwrap();
    tick();
    s.set_status(idea, Status::Dropped, Some("no"), &Actor::Me)
        .unwrap();
    assert_eq!(since(&s, c), newest_at(&s, idea));
}

#[test]
fn removing_the_last_open_blocker_makes_a_task_ready_since_then() {
    let mut s = store();
    let [a, b, c] = ["A", "B", "C"].map(|t| top(&mut s, t));
    s.add_dependency(c, a, &Actor::Me).unwrap();
    s.add_dependency(c, b, &Actor::Me).unwrap();
    set_status(&mut s, a, Status::Done);
    tick();
    s.remove_dependency(c, b, &Actor::Me).unwrap();
    assert_eq!(since(&s, c), newest_at(&s, c));
}
