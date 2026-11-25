//! Dependencies, loops, waiting and run order (rules 14–18).
#![allow(clippy::unwrap_used)]

use airboss_core::{Actor, Error, EventKind, NewTask, RunOrder, Store, Task, TaskId};

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn top(store: &mut Store, title: &str) -> TaskId {
    store.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn under(store: &mut Store, parent: TaskId, title: &str) -> TaskId {
    store
        .add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
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
    assert_eq!(ids(&s.ready().unwrap()), vec![a]);
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
    assert_eq!(ids(&s.ready().unwrap()), vec![a, b]);
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

    let ready = ids(&s.ready().unwrap());
    assert_eq!(ready, vec![p, p1, q]);
    assert!(!ready.contains(&proposal), "proposals are not ready");
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
    assert_eq!(ids(&order.waiting_outside), vec![a, c]);
}

#[test]
fn a_loop_made_only_by_lifting_is_rejected() {
    let mut s = store();
    let p = top(&mut s, "Project");
    let x = under(&mut s, p, "X");
    let y = under(&mut s, p, "Y");
    let [x1, x2] = ["X1", "X2"].map(|t| under(&mut s, x, t));
    let [y1, y2] = ["Y1", "Y2"].map(|t| under(&mut s, y, t));
    // No loop between tasks, but X waits on Y and Y waits on X.
    s.add_dependency(x1, y1, &Actor::Me).unwrap();
    s.add_dependency(y2, x2, &Actor::Me).unwrap();
    assert!(matches!(s.run_order(p), Err(Error::Loop)));
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

// ---- Early performance check (run with `--release -- --ignored perf`) ----

/// A tiny self-cleaning temp dir, so the perf check needs no extra crate.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!("airboss-core-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A tiny linear congruential generator: repeatable, and no `rand` crate.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

/// Adds random dependencies, each from a later task to an earlier one so no loop forms,
/// until `count` have landed. Ancestor/descendant pairs are refused and skipped.
fn random_deps(s: &mut Store, tasks: &[TaskId], rng: &mut Lcg, count: usize) -> Vec<f64> {
    let mut times = Vec::with_capacity(count);
    while times.len() < count {
        let (i, j) = (rng.below(tasks.len()), rng.below(tasks.len()));
        let (later, earlier) = (i.max(j), i.min(j));
        if later == earlier {
            continue;
        }
        let start = std::time::Instant::now();
        let out = s.add_dependency(tasks[later], tasks[earlier], &Actor::Me);
        let took = start.elapsed().as_secs_f64() * 1000.0;
        match out {
            Ok(()) => times.push(took),
            Err(Error::InvalidTarget) => {}
            Err(e) => panic!("unexpected {e:?}"),
        }
    }
    times
}

#[test]
#[ignore]
fn perf_add_dependency_on_2000_tasks() {
    let dir = TempDir::new("perf-deps");
    let mut s = Store::open(&dir.0.join("airboss.db")).unwrap();
    let mut rng = Lcg(42);
    let mut tasks: Vec<TaskId> = Vec::with_capacity(2000);
    for i in 0..2000 {
        // A random earlier task as parent, or top level one time in ten.
        let new = NewTask::new(format!("Task {i}"));
        let new = if tasks.is_empty() || rng.below(10) == 0 {
            new
        } else {
            new.under(tasks[rng.below(tasks.len())])
        };
        tasks.push(s.add_task(new, &Actor::Me).unwrap().id);
    }
    random_deps(&mut s, &tasks, &mut rng, 1000);

    let mut times = random_deps(&mut s, &tasks, &mut rng, 200);
    times.sort_by(f64::total_cmp);
    let p95 = times[times.len() * 95 / 100];
    println!("add_dependency on 2,000 tasks / 1,000 deps: p95 {p95:.3} ms over 200 runs");
    assert!(p95 < 2.0, "p95 {p95:.3} ms");
}
