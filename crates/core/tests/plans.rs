//! Plans arriving as trees (rules 19, 20 and 21).
#![allow(clippy::unwrap_used)]

use std::path::Path;

use airboss_core::{
    Actor, DepRef, Error, Event, EventKind, NewTask, Origin, PlanTask, Status, Store, Task, TaskId,
};

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn top(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn children(s: &Store, parent: Option<TaskId>) -> Vec<Task> {
    s.tree()
        .unwrap()
        .into_iter()
        .map(|n| n.task)
        .filter(|t| t.parent == parent)
        .collect()
}

fn titles(tasks: &[Task]) -> Vec<&str> {
    tasks.iter().map(|t| t.title.as_str()).collect()
}

fn ids(tasks: &[Task]) -> Vec<TaskId> {
    tasks.iter().map(|t| t.id).collect()
}

/// Every event in the store, oldest first, by walking every task's history.
fn all_events(s: &Store) -> Vec<Event> {
    let mut events: Vec<Event> = s
        .tree()
        .unwrap()
        .into_iter()
        .flat_map(|n| s.history(n.task.id).unwrap())
        .collect();
    events.sort_by_key(|e| e.id);
    events
}

/// A snapshot of everything visible: the tree and all events.
fn snapshot(s: &Store) -> (Vec<Task>, Vec<Event>) {
    (
        s.tree().unwrap().into_iter().map(|n| n.task).collect(),
        all_events(s),
    )
}

/// Phases with steps under them; the build steps wait on design and the ship step on both.
fn product_plan() -> Vec<PlanTask> {
    vec![
        PlanTask::new("design", "Design")
            .child(PlanTask::new("design.sketch", "Sketch screens"))
            .child(PlanTask::new("design.review", "Review sketches").after("design.sketch")),
        PlanTask::new("build", "Build")
            .child(PlanTask::new("build.api", "Build API").after("design.review"))
            .child(
                PlanTask::new("build.ui", "Build UI")
                    .after("design.review")
                    .after("build.api"),
            ),
        PlanTask::new("ship", "Ship").after("build"),
    ]
}

#[test]
fn a_three_level_plan_is_created_with_its_dependencies() {
    let mut s = store();
    let root = top(&mut s, "Product");
    top(&mut s, "Unrelated");
    let r = s
        .add_tasks(Some(root), &product_plan(), &Actor::Plan)
        .unwrap();

    let phases = children(&s, Some(root));
    assert_eq!(titles(&phases), vec!["Design", "Build", "Ship"]);
    assert_eq!(
        phases.iter().map(|t| t.position).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    let design = children(&s, Some(phases[0].id));
    let build = children(&s, Some(phases[1].id));
    assert_eq!(titles(&design), vec!["Sketch screens", "Review sketches"]);
    assert_eq!(titles(&build), vec!["Build API", "Build UI"]);
    assert_eq!(
        build.iter().map(|t| t.position).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(children(&s, Some(phases[2].id)).is_empty());

    let all: Vec<Task> = phases
        .iter()
        .chain(&design)
        .chain(&build)
        .cloned()
        .collect();
    assert_eq!(r.created.len(), 7);
    assert!(r.existing.is_empty());
    for t in &all {
        assert_eq!(t.status, Status::Todo);
        assert_eq!(t.origin, Origin::Plan);
        assert_eq!(r.id(t.plan_key.as_deref().unwrap()), Some(t.id));
    }
    // Plan order is depth-first.
    assert_eq!(
        r.created,
        vec![
            phases[0].id,
            design[0].id,
            design[1].id,
            phases[1].id,
            build[0].id,
            build[1].id,
            phases[2].id
        ]
    );

    assert_eq!(ids(&s.waits_on(design[1].id).unwrap()), vec![design[0].id]);
    assert_eq!(
        ids(&s.waits_on(build[1].id).unwrap()),
        vec![design[1].id, build[0].id]
    );
    assert_eq!(ids(&s.waits_on(phases[2].id).unwrap()), vec![phases[1].id]);

    // One `created` event each, naming its dependencies, and nothing else.
    for t in &all {
        let h = s.history(t.id).unwrap();
        assert_eq!(h.len(), 1, "{}", t.title);
        assert_eq!(h[0].kind, EventKind::Created);
        assert_eq!(h[0].actor, Actor::Plan);
    }
    let ui_created = &s.history(build[1].id).unwrap()[0];
    assert_eq!(
        ui_created.body.as_deref(),
        Some(format!("waits on #{}, #{}", design[1].id.0, build[0].id.0).as_str())
    );
    assert_eq!(s.history(design[0].id).unwrap()[0].body, None);
}

#[test]
fn the_same_plan_twice_changes_nothing() {
    let mut s = store();
    let root = top(&mut s, "Product");
    let first = s
        .add_tasks(Some(root), &product_plan(), &Actor::Plan)
        .unwrap();
    let before = snapshot(&s);

    let second = s
        .add_tasks(Some(root), &product_plan(), &Actor::Plan)
        .unwrap();
    assert!(second.created.is_empty());
    assert_eq!(second.existing, first.created);
    assert_eq!(second.keys, first.keys);
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_revised_plan_adds_only_its_new_step_and_keeps_old_titles() {
    let mut s = store();
    let root = top(&mut s, "Product");
    let first = s
        .add_tasks(Some(root), &product_plan(), &Actor::Plan)
        .unwrap();
    let events_before = all_events(&s).len();

    let mut revised = product_plan();
    revised[1].children[0].title = "Build the API, renamed".into();
    revised[1]
        .children
        .push(PlanTask::new("build.docs", "Write docs").after("build.api"));
    let r = s.add_tasks(Some(root), &revised, &Actor::Plan).unwrap();

    let docs = r.id("build.docs").unwrap();
    assert_eq!(r.created, vec![docs]);
    assert_eq!(r.existing, first.created);
    let api = first.id("build.api").unwrap();
    assert_eq!(s.task(api).unwrap().title, "Build API");
    let build = children(&s, first.id("build"));
    assert_eq!(titles(&build), vec!["Build API", "Build UI", "Write docs"]);
    assert_eq!(build[2].position, 3);
    assert_eq!(ids(&s.waits_on(docs).unwrap()), vec![api]);
    // Only the new step's `created` event.
    let events = all_events(&s);
    assert_eq!(events.len(), events_before + 1);
    assert_eq!(events[events_before].task, docs);
}

#[test]
fn a_new_dependency_on_an_existing_step_gets_a_waits_on_added_event() {
    let mut s = store();
    let root = top(&mut s, "Product");
    let first = s
        .add_tasks(Some(root), &product_plan(), &Actor::Plan)
        .unwrap();
    let sketch = first.id("design.sketch").unwrap();

    let mut revised = product_plan();
    revised.push(PlanTask::new("research", "Research"));
    revised[0].children[0] = PlanTask::new("design.sketch", "Sketch screens").after("research");
    let r = s.add_tasks(Some(root), &revised, &Actor::Plan).unwrap();

    let research = r.id("research").unwrap();
    assert_eq!(r.created, vec![research]);
    assert_eq!(ids(&s.waits_on(sketch).unwrap()), vec![research]);
    let h = s.history(sketch).unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h[1].kind, EventKind::WaitsOnAdded);
    assert_eq!(
        h[1].body.as_deref(),
        Some(format!("waits on #{}", research.0).as_str())
    );
    assert_eq!(s.history(research).unwrap().len(), 1);
}

#[test]
fn a_plan_with_a_dependency_loop_writes_nothing() {
    let mut s = store();
    let root = top(&mut s, "Product");
    let before = snapshot(&s);
    let plan = vec![
        PlanTask::new("a", "A").after("b"),
        PlanTask::new("b", "B").after("a"),
    ];
    assert!(matches!(
        s.add_tasks(Some(root), &plan, &Actor::Plan),
        Err(Error::Loop)
    ));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_loop_through_an_existing_task_writes_nothing() {
    let mut s = store();
    let root = top(&mut s, "Product");
    let first = s
        .add_tasks(Some(root), &[PlanTask::new("a", "A")], &Actor::Plan)
        .unwrap();
    let a = first.id("a").unwrap();
    let before = snapshot(&s);
    // A already exists; the revised plan makes A wait on B and B wait on A.
    let plan = vec![
        PlanTask::new("a", "A").after("b"),
        PlanTask::new("b", "B").after_task(a),
    ];
    assert!(matches!(
        s.add_tasks(Some(root), &plan, &Actor::Plan),
        Err(Error::Loop)
    ));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_step_waiting_on_its_own_ancestor_is_an_invalid_target() {
    let mut s = store();
    let plan =
        vec![PlanTask::new("phase", "Phase").child(PlanTask::new("step", "Step").after("phase"))];
    assert!(matches!(
        s.add_tasks(None, &plan, &Actor::Me),
        Err(Error::InvalidTarget)
    ));
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn a_step_waiting_on_itself_is_a_loop() {
    let mut s = store();
    let plan = vec![PlanTask::new("a", "A").after("a")];
    assert!(matches!(
        s.add_tasks(None, &plan, &Actor::Me),
        Err(Error::Loop)
    ));
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn a_key_used_twice_is_a_duplicate_key() {
    let mut s = store();
    let plan = vec![
        PlanTask::new("a", "A").child(PlanTask::new("x", "X")),
        PlanTask::new("b", "B").child(PlanTask::new("x", "Another X")),
    ];
    match s.add_tasks(None, &plan, &Actor::Me) {
        Err(Error::DuplicateKey(k)) => assert_eq!(k, "x"),
        other => panic!("expected DuplicateKey, got {other:?}"),
    }
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn an_unknown_key_dependency_is_not_found() {
    let mut s = store();
    let plan = vec![PlanTask::new("a", "A").after("nope")];
    assert!(matches!(
        s.add_tasks(None, &plan, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn a_missing_task_dependency_is_not_found() {
    let mut s = store();
    let plan = vec![PlanTask::new("a", "A").after_task(TaskId(999))];
    assert!(matches!(
        s.add_tasks(None, &plan, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn a_blank_title_or_missing_repo_writes_nothing() {
    let mut s = store();
    let blank = vec![
        PlanTask::new("a", "A"),
        PlanTask::new("b", "B").child(PlanTask::new("c", "  ")),
    ];
    assert!(matches!(
        s.add_tasks(None, &blank, &Actor::Me),
        Err(Error::EmptyTitle)
    ));
    let no_repo = vec![PlanTask::new("a", "A").in_repo(airboss_core::RepoId(42))];
    assert!(matches!(
        s.add_tasks(None, &no_repo, &Actor::Me),
        Err(Error::NotFound)
    ));
    assert!(s.tree().unwrap().is_empty());
}

#[test]
fn steps_keep_their_body_and_repo() {
    let mut s = store();
    let repo = s
        .add_repo("airboss", Path::new("/src/airboss"), None, &Actor::Me)
        .unwrap();
    let plan = vec![
        PlanTask::new("a", "  A  ")
            .with_body("why")
            .in_repo(repo.id),
    ];
    let r = s.add_tasks(None, &plan, &Actor::Me).unwrap();
    let a = s.task(r.id("a").unwrap()).unwrap();
    assert_eq!(
        (a.title.as_str(), a.body.as_deref(), a.repo),
        ("A", Some("why"), Some(repo.id))
    );
}

#[test]
fn a_dependency_on_an_existing_task_outside_the_plan_works() {
    let mut s = store();
    let outside = top(&mut s, "Outside");
    let root = top(&mut s, "Product");
    let plan = vec![PlanTask::new("a", "A").after_task(outside)];
    let r = s.add_tasks(Some(root), &plan, &Actor::Plan).unwrap();
    let a = r.id("a").unwrap();
    assert_eq!(ids(&s.waits_on(a).unwrap()), vec![outside]);
    assert_eq!(
        s.history(a).unwrap()[0].body.as_deref(),
        Some(format!("waits on #{}", outside.0).as_str())
    );
    // The outside task isn't touched.
    assert_eq!(s.history(outside).unwrap().len(), 1);
}

#[test]
fn a_plan_under_a_done_parent_is_an_invalid_target() {
    let mut s = store();
    let root = top(&mut s, "Product");
    s.set_status(root, Status::Done, None, &Actor::Me).unwrap();
    let before = snapshot(&s);
    assert!(matches!(
        s.add_tasks(Some(root), &product_plan(), &Actor::Plan),
        Err(Error::InvalidTarget)
    ));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_plan_under_a_missing_parent_is_not_found() {
    let mut s = store();
    assert!(matches!(
        s.add_tasks(Some(TaskId(7)), &product_plan(), &Actor::Plan),
        Err(Error::NotFound)
    ));
}

#[test]
fn new_steps_under_a_finished_existing_step_are_an_invalid_target() {
    let mut s = store();
    let first = s
        .add_tasks(None, &[PlanTask::new("a", "A")], &Actor::Plan)
        .unwrap();
    s.set_status(first.id("a").unwrap(), Status::Done, None, &Actor::Me)
        .unwrap();
    let before = snapshot(&s);
    let revised = vec![PlanTask::new("a", "A").child(PlanTask::new("b", "B"))];
    assert!(matches!(
        s.add_tasks(None, &revised, &Actor::Plan),
        Err(Error::InvalidTarget)
    ));
    assert_eq!(snapshot(&s), before);
}

#[test]
fn a_top_level_plan_goes_after_existing_top_level_tasks() {
    let mut s = store();
    top(&mut s, "Existing");
    let r = s.add_tasks(None, &product_plan(), &Actor::Me).unwrap();
    let tops = children(&s, None);
    assert_eq!(titles(&tops), vec!["Existing", "Design", "Build", "Ship"]);
    assert_eq!(
        tops.iter().map(|t| t.position).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(r.created.len(), 7);
    // And running it again at the top level resolves every key too.
    let again = s.add_tasks(None, &product_plan(), &Actor::Me).unwrap();
    assert!(again.created.is_empty());
    assert_eq!(again.existing, r.created);
}

#[test]
fn only_me_and_plan_may_add_a_plan() {
    for actor in [
        Actor::Agent("t3/a1".into()),
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let mut s = store();
        assert!(
            matches!(
                s.add_tasks(None, &product_plan(), &actor),
                Err(Error::NotAllowed)
            ),
            "{actor}"
        );
        assert!(s.tree().unwrap().is_empty());
    }
    for actor in [Actor::Me, Actor::Plan] {
        let mut s = store();
        assert!(
            s.add_tasks(None, &product_plan(), &actor).is_ok(),
            "{actor}"
        );
    }
}

#[test]
fn dep_refs_mix_keys_and_task_ids() {
    let step = PlanTask::new("a", "A").after("b").after_task(TaskId(3));
    assert_eq!(
        step.depends_on,
        vec![DepRef::Key("b".into()), DepRef::Task(TaskId(3))]
    );
}

/// A real plan as a fixture: a four-phase migration. Phase 2's steps wait on Phase 1's, and
/// Phases 3 and 4 each wait on a Phase 2 step, so the phases run in three waves.
#[test]
fn a_migration_plan_runs_in_the_expected_waves() {
    let mut s = store();
    let migration = top(&mut s, "Migrate billing to the new ledger");
    let plan = vec![
        PlanTask::new("p1", "Phase 1: Prepare")
            .child(PlanTask::new("p1.schema", "Add ledger tables"))
            .child(PlanTask::new("p1.flag", "Add the dual-write flag")),
        PlanTask::new("p2", "Phase 2: Dual write")
            .child(
                PlanTask::new("p2.write", "Write to both stores")
                    .after("p1.schema")
                    .after("p1.flag"),
            )
            .child(PlanTask::new("p2.backfill", "Backfill history").after("p2.write")),
        PlanTask::new("p3", "Phase 3: Switch reads")
            .child(PlanTask::new("p3.reads", "Read from the ledger").after("p2.backfill")),
        PlanTask::new("p4", "Phase 4: Reporting")
            .child(PlanTask::new("p4.reports", "Rebuild reports").after("p2.backfill")),
    ];
    let r = s.add_tasks(Some(migration), &plan, &Actor::Plan).unwrap();
    let phase = |k: &str| r.id(k).unwrap();

    let order = s.run_order(migration).unwrap();
    let waves: Vec<Vec<TaskId>> = order.waves.iter().map(|w| ids(w)).collect();
    assert_eq!(
        waves,
        vec![
            vec![phase("p1")],
            vec![phase("p2")],
            vec![phase("p3"), phase("p4")]
        ]
    );
    assert!(order.waiting_outside.is_empty());

    // Inside Phase 1 both steps can start at once.
    let inner = s.run_order(phase("p1")).unwrap();
    assert_eq!(
        inner.waves.iter().map(|w| ids(w)).collect::<Vec<_>>(),
        vec![vec![phase("p1.schema"), phase("p1.flag")]]
    );
    // Inside Phase 2 everything waits, directly or not, on Phase 1, outside it.
    let inner = s.run_order(phase("p2")).unwrap();
    assert!(inner.waves.is_empty());
    assert_eq!(
        ids(&inner.waiting_outside),
        vec![phase("p2.write"), phase("p2.backfill")]
    );
}
