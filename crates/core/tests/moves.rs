//! Moving tasks: rules 11, 12 and 13.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use airboss_core::{Actor, Error, EventKind, NewTask, PlanTask, Status, Store, TaskId};

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

fn add(store: &mut Store, title: &str, parent: Option<TaskId>) -> TaskId {
    let new = NewTask::new(title);
    let new = match parent {
        Some(p) => new.under(p),
        None => new,
    };
    store.add_task(new, &Actor::Me).unwrap().id
}

/// Adds `n` children named `prefix1..prefixN` under `parent`.
fn children(store: &mut Store, prefix: &str, parent: Option<TaskId>, n: usize) -> Vec<TaskId> {
    (1..=n)
        .map(|i| add(store, &format!("{prefix}{i}"), parent))
        .collect()
}

/// The ids in `parent`'s group, in position order.
fn group(store: &Store, parent: Option<TaskId>) -> Vec<TaskId> {
    let mut kids: Vec<_> = store
        .tree()
        .unwrap()
        .into_iter()
        .map(|n| n.task)
        .filter(|t| t.parent == parent)
        .collect();
    kids.sort_by_key(|t| t.position);
    kids.into_iter().map(|t| t.id).collect()
}

/// Every sibling group's positions, keyed by parent.
fn groups(store: &Store) -> BTreeMap<Option<TaskId>, Vec<u32>> {
    let mut out: BTreeMap<Option<TaskId>, Vec<u32>> = BTreeMap::new();
    for n in store.tree().unwrap() {
        out.entry(n.task.parent).or_default().push(n.task.position);
    }
    out
}

fn assert_gap_free(store: &Store) {
    for (parent, mut positions) in groups(store) {
        positions.sort_unstable();
        let expected: Vec<u32> = (1..=positions.len() as u32).collect();
        assert_eq!(positions, expected, "group under {parent:?} has gaps");
    }
}

fn moved(store: &mut Store, id: TaskId, parent: Option<TaskId>, position: u32) {
    store.move_task(id, parent, position, &Actor::Me).unwrap();
}

// ---- Rule 13: positions ----

#[test]
fn moving_to_the_top_of_a_group() {
    let mut s = store();
    let t = children(&mut s, "t", None, 4);
    moved(&mut s, t[3], None, 1);
    assert_eq!(group(&s, None), vec![t[3], t[0], t[1], t[2]]);
    assert_gap_free(&s);
}

#[test]
fn moving_to_the_middle_of_a_group() {
    let mut s = store();
    let t = children(&mut s, "t", None, 5);
    moved(&mut s, t[0], None, 3);
    assert_eq!(group(&s, None), vec![t[1], t[2], t[0], t[3], t[4]]);
    moved(&mut s, t[4], None, 2);
    assert_eq!(group(&s, None), vec![t[1], t[4], t[2], t[0], t[3]]);
    assert_gap_free(&s);
}

#[test]
fn moving_to_the_end_of_a_group() {
    let mut s = store();
    let t = children(&mut s, "t", None, 4);
    moved(&mut s, t[0], None, 4);
    assert_eq!(group(&s, None), vec![t[1], t[2], t[3], t[0]]);
    assert_gap_free(&s);
}

#[test]
fn moving_between_groups_renumbers_both() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    let ak = children(&mut s, "a", Some(a), 4);
    let bk = children(&mut s, "b", Some(b), 3);

    moved(&mut s, ak[1], Some(b), 2);

    assert_eq!(group(&s, Some(a)), vec![ak[0], ak[2], ak[3]]);
    assert_eq!(group(&s, Some(b)), vec![bk[0], ak[1], bk[1], bk[2]]);
    let t = s.task(ak[1]).unwrap();
    assert_eq!((t.parent, t.position), (Some(b), 2));
    assert_gap_free(&s);
}

#[test]
fn a_moved_task_takes_its_subtree_with_it() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    let child = add(&mut s, "child", Some(a));
    let grandchild = add(&mut s, "grandchild", Some(child));

    moved(&mut s, child, Some(b), 1);

    assert_eq!(s.task(grandchild).unwrap().parent, Some(child));
    assert_eq!(s.task(child).unwrap().parent, Some(b));
    assert!(group(&s, Some(a)).is_empty());
}

#[test]
fn position_zero_is_clamped_to_one() {
    let mut s = store();
    let t = children(&mut s, "t", None, 3);
    moved(&mut s, t[2], None, 0);
    assert_eq!(group(&s, None), vec![t[2], t[0], t[1]]);
    assert_eq!(s.task(t[2]).unwrap().position, 1);
}

#[test]
fn a_position_past_the_end_is_clamped_to_the_end() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let ak = children(&mut s, "a", Some(a), 2);
    let loner = add(&mut s, "loner", None);

    // Within the same group.
    moved(&mut s, ak[0], Some(a), 99);
    assert_eq!(group(&s, Some(a)), vec![ak[1], ak[0]]);
    assert_eq!(s.task(ak[0]).unwrap().position, 2);

    // Into another group.
    moved(&mut s, loner, Some(a), u32::MAX);
    assert_eq!(group(&s, Some(a)), vec![ak[1], ak[0], loner]);
    assert_eq!(s.task(loner).unwrap().position, 3);
    assert_gap_free(&s);
}

#[test]
fn moving_to_the_top_level_and_back() {
    let mut s = store();
    let top = children(&mut s, "top", None, 2);
    let kids = children(&mut s, "k", Some(top[0]), 3);

    moved(&mut s, kids[1], None, 1);
    assert_eq!(group(&s, None), vec![kids[1], top[0], top[1]]);
    assert_eq!(group(&s, Some(top[0])), vec![kids[0], kids[2]]);
    assert_eq!(s.task(kids[1]).unwrap().parent, None);
    assert_gap_free(&s);

    moved(&mut s, kids[1], Some(top[0]), 2);
    assert_eq!(group(&s, None), vec![top[0], top[1]]);
    assert_eq!(group(&s, Some(top[0])), kids);
    assert_gap_free(&s);
}

#[test]
fn a_task_added_after_a_move_lands_at_the_end() {
    let mut s = store();
    let t = children(&mut s, "t", None, 3);
    moved(&mut s, t[0], None, 3);
    let late = add(&mut s, "late", None);
    assert_eq!(s.task(late).unwrap().position, 4);
    assert_gap_free(&s);
}

// ---- Rule 11: no cycles ----

#[test]
fn a_task_cannot_move_under_itself() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let err = s.move_task(a, Some(a), 1, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::Loop), "{err:?}");
}

#[test]
fn a_task_cannot_move_into_its_own_subtree() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", Some(a));
    let c = add(&mut s, "C", Some(b));
    let before = groups(&s);

    for target in [b, c] {
        let err = s.move_task(a, Some(target), 1, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::Loop), "{err:?}");
    }
    assert_eq!(groups(&s), before);
    assert_eq!(s.history(a).unwrap().len(), 1);
}

// ---- Rule 15: a move can't make a task wait on its own ancestor or descendant ----

#[test]
fn moving_a_task_under_what_it_waits_on_is_rejected() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    s.add_dependency(a, b, &Actor::Me).unwrap();
    let before = snapshot(&s);

    let err = s.move_task(a, Some(b), 1, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    assert_eq!(snapshot(&s), before);
}

#[test]
fn moving_a_task_under_what_its_subtree_blocks_is_rejected() {
    let mut s = store();
    let c = add(&mut s, "C", None);
    let d = add(&mut s, "D", Some(c));
    let e = add(&mut s, "E", None);
    let f = add(&mut s, "F", Some(e));
    s.add_dependency(e, d, &Actor::Me).unwrap();
    let before = snapshot(&s);

    // Directly under E, and deeper, under E's child F.
    for target in [e, f] {
        let err = s.move_task(c, Some(target), 1, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    }
    assert_eq!(snapshot(&s), before);
}

#[test]
fn moves_without_such_a_dependency_still_work() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let b = add(&mut s, "B", None);
    let c = add(&mut s, "C", None);
    let d = add(&mut s, "D", Some(c));
    s.add_dependency(a, b, &Actor::Me).unwrap();
    s.add_dependency(d, b, &Actor::Me).unwrap();

    // C's subtree waits on B, but B isn't C's new parent or one of its ancestors.
    moved(&mut s, c, Some(a), 1);
    assert_eq!(s.task(c).unwrap().parent, Some(a));
    // A task with no dependencies moves under B freely.
    let free = add(&mut s, "Free", None);
    moved(&mut s, free, Some(b), 1);
    assert_eq!(s.task(free).unwrap().parent, Some(b));
}

// ---- Rule 20: plan keys stay unique under a parent ----

#[test]
fn moving_a_plan_task_next_to_one_with_the_same_key_is_already_exists() {
    let mut s = store();
    let p1 = add(&mut s, "P1", None);
    let p2 = add(&mut s, "P2", None);
    let k1 = s
        .add_tasks(Some(p1), &[PlanTask::new("k", "K1")], &Actor::Plan)
        .unwrap()
        .id("k")
        .unwrap();
    s.add_tasks(Some(p2), &[PlanTask::new("k", "K2")], &Actor::Plan)
        .unwrap();
    s.add_tasks(None, &[PlanTask::new("k", "Top K")], &Actor::Plan)
        .unwrap();
    let before = snapshot(&s);

    for target in [Some(p2), None] {
        let err = s.move_task(k1, target, 1, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::AlreadyExists), "{target:?}: {err:?}");
    }
    assert_eq!(snapshot(&s), before);

    // Within its own group it still moves.
    let other = add(&mut s, "Other", Some(p1));
    moved(&mut s, other, Some(p1), 1);
    assert_eq!(group(&s, Some(p1)), vec![other, k1]);
}

// ---- Rule 12: no moving under closed or proposed parents ----

#[test]
fn moving_under_a_proposed_parent_is_rejected() {
    let mut s = store();
    let proposal = s
        .add_task(NewTask::new("Idea"), &Actor::Agent("t3/x".into()))
        .unwrap()
        .id;
    let t = add(&mut s, "T", None);
    let err = s.move_task(t, Some(proposal), 1, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    assert_eq!(s.task(t).unwrap().parent, None);
}

#[test]
fn adding_under_a_proposed_parent_is_rejected() {
    let mut s = store();
    let proposal = s
        .add_task(NewTask::new("Idea"), &Actor::Agent("t3/x".into()))
        .unwrap()
        .id;
    let err = s
        .add_task(NewTask::new("Child").under(proposal), &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
}

fn set_status(store: &mut Store, id: TaskId, to: Status) {
    store.set_status(id, to, None, &Actor::Me).unwrap();
}

/// Every task's (id, parent, position) and how many events it has.
fn snapshot(store: &Store) -> Vec<(TaskId, Option<TaskId>, u32, usize)> {
    let mut out: Vec<_> = store
        .tree()
        .unwrap()
        .into_iter()
        .map(|n| {
            let t = n.task;
            (
                t.id,
                t.parent,
                t.position,
                store.history(t.id).unwrap().len(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn moving_under_a_done_or_dropped_parent_is_rejected() {
    for status in [Status::Done, Status::Dropped] {
        let mut s = store();
        let closed = add(&mut s, "Closed", None);
        let t = add(&mut s, "T", None);
        set_status(&mut s, closed, status);
        let before = snapshot(&s);

        let err = s.move_task(t, Some(closed), 1, &Actor::Me).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{status:?}: {err:?}");
        assert_eq!(snapshot(&s), before);
    }
}

#[test]
fn adding_under_a_done_or_dropped_parent_is_rejected() {
    for status in [Status::Done, Status::Dropped] {
        let mut s = store();
        let closed = add(&mut s, "Closed", None);
        set_status(&mut s, closed, status);
        let before = snapshot(&s);

        let err = s
            .add_task(NewTask::new("Child").under(closed), &Actor::Me)
            .unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{status:?}: {err:?}");
        assert_eq!(snapshot(&s), before);
    }
}

#[test]
fn moving_and_adding_under_other_statuses_is_fine() {
    for status in [
        Status::Todo,
        Status::Doing,
        Status::InReview,
        Status::Blocked,
    ] {
        let mut s = store();
        let p = add(&mut s, "P", None);
        let t = add(&mut s, "T", None);
        s.set_status(p, status, Some("reason"), &Actor::Me).unwrap();
        s.move_task(t, Some(p), 1, &Actor::Me).unwrap();
        add(&mut s, "U", Some(p));
    }
}

#[test]
fn moving_under_a_missing_parent_is_not_found() {
    let mut s = store();
    let t = add(&mut s, "T", None);
    let err = s
        .move_task(t, Some(TaskId(999)), 1, &Actor::Me)
        .unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

#[test]
fn moving_a_missing_task_is_not_found() {
    let mut s = store();
    let err = s.move_task(TaskId(999), None, 1, &Actor::Me).unwrap_err();
    assert!(matches!(err, Error::NotFound), "{err:?}");
}

// ---- Rule 8: only `me` moves ----

#[test]
fn only_me_may_move_a_task() {
    let mut s = store();
    let t = children(&mut s, "t", None, 2);
    for actor in [
        Actor::Agent("t3/a1".into()),
        Actor::Plan,
        Actor::SyncGh,
        Actor::SyncT3,
        Actor::Launch,
    ] {
        let err = s.move_task(t[1], None, 1, &actor).unwrap_err();
        assert!(matches!(err, Error::NotAllowed), "{actor}: {err:?}");
    }
    assert_eq!(group(&s, None), t);
}

// ---- Rule 13 / 21: events ----

#[test]
fn a_move_writes_one_moved_event_on_the_moved_task_only() {
    let mut s = store();
    let a = add(&mut s, "A", None);
    let ak = children(&mut s, "a", Some(a), 3);
    let top = children(&mut s, "top", None, 2);
    let all: Vec<TaskId> = [vec![a], ak.clone(), top.clone()].concat();
    let before: BTreeMap<TaskId, usize> = all
        .iter()
        .map(|&id| (id, s.history(id).unwrap().len()))
        .collect();

    moved(&mut s, ak[0], None, 1);

    for &id in &all {
        let history = s.history(id).unwrap();
        if id == ak[0] {
            assert_eq!(history.len(), before[&id] + 1);
            let last = history.last().unwrap();
            assert_eq!((last.kind, &last.actor), (EventKind::Moved, &Actor::Me));
        } else {
            assert_eq!(history.len(), before[&id], "sibling {id:?} got an event");
        }
    }
}

// ---- Property: 500 random moves keep every group 1..n ----

/// A tiny linear congruential generator, so the test needs no `rand`.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn five_hundred_random_moves_keep_every_group_gap_free() {
    let mut s = store();
    let mut rng = Lcg(0x5eed_a1b0);
    let mut ids: Vec<TaskId> = Vec::new();
    for i in 0..60 {
        let parent = if ids.is_empty() || rng.below(3) == 0 {
            None
        } else {
            Some(ids[rng.below(ids.len())])
        };
        ids.push(add(&mut s, &format!("task {i}"), parent));
    }
    let everyone: BTreeSet<TaskId> = ids.iter().copied().collect();
    assert_gap_free(&s);

    let (mut ok, mut loops) = (0, 0);
    for step in 0..500 {
        let id = ids[rng.below(ids.len())];
        let parent = match rng.below(5) {
            0 => None,
            _ => Some(ids[rng.below(ids.len())]),
        };
        let others = group(&s, parent).into_iter().filter(|&t| t != id).count();
        let position = rng.below(others + 4) as u32;
        let before = groups(&s);

        match s.move_task(id, parent, position, &Actor::Me) {
            Ok(()) => {
                ok += 1;
                let t = s.task(id).unwrap();
                let expected = (position.max(1) as usize).min(others + 1) as u32;
                assert_eq!((t.parent, t.position), (parent, expected), "step {step}");
            }
            Err(Error::Loop) => {
                loops += 1;
                assert_eq!(
                    groups(&s),
                    before,
                    "step {step}: a rejected move changed things"
                );
            }
            Err(e) => panic!("step {step}: unexpected {e:?}"),
        }

        assert_gap_free(&s);
        let now: BTreeSet<TaskId> = s.tree().unwrap().iter().map(|n| n.task.id).collect();
        assert_eq!(now, everyone, "step {step}: the set of tasks changed");
    }
    // The walk should exercise both paths.
    assert!(ok > 100 && loops > 10, "ok = {ok}, loops = {loops}");
}
