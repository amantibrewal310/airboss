//! Building and reading the task tree.
#![allow(clippy::unwrap_used)]

use airboss_core::{Actor, NewTask, Status, Store};

fn titled(title: &str) -> NewTask {
    NewTask::new(title)
}

#[test]
fn a_new_task_starts_as_todo_at_the_end_of_its_siblings() {
    let mut store = Store::open_in_memory().unwrap();
    let a = store.add_task(titled("Fix typo"), &Actor::Me).unwrap();
    let b = store.add_task(titled("Booking flow"), &Actor::Me).unwrap();

    assert_eq!(a.status, Status::Todo);
    assert_eq!((a.position, b.position), (1, 2));
    assert_eq!(b.parent, None);
}

#[test]
fn tasks_nest_five_levels_deep_and_the_tree_reads_back_depth_first() {
    let mut store = Store::open_in_memory().unwrap();
    let mut parent = None;
    let mut ids = vec![];
    for level in 1..=5 {
        let new = titled(&format!("Level {level}"));
        let new = match parent {
            Some(p) => new.under(p),
            None => new,
        };
        let t = store.add_task(new, &Actor::Me).unwrap();
        ids.push(t.id);
        parent = Some(t.id);
    }
    let sibling = store
        .add_task(titled("Second top-level"), &Actor::Me)
        .unwrap();

    // Top-level tasks and nested tasks are numbered separately.
    assert_eq!(sibling.position, 2);

    let tree = store.tree().unwrap();
    let order: Vec<_> = tree
        .iter()
        .map(|n| (n.task.title.as_str(), n.depth))
        .collect();
    assert_eq!(
        order,
        vec![
            ("Level 1", 0),
            ("Level 2", 1),
            ("Level 3", 2),
            ("Level 4", 3),
            ("Level 5", 4),
            ("Second top-level", 0),
        ]
    );
    assert_eq!(tree[4].task.parent, Some(ids[3]));
    assert_eq!(tree[5].task.id, sibling.id);
}

#[test]
fn children_get_consecutive_positions_under_their_parent() {
    let mut store = Store::open_in_memory().unwrap();
    let p = store.add_task(titled("Parent"), &Actor::Me).unwrap();
    for t in ["A", "B", "C"] {
        store.add_task(titled(t).under(p.id), &Actor::Me).unwrap();
    }
    let children: Vec<_> = store
        .tree()
        .unwrap()
        .into_iter()
        .skip(1)
        .map(|n| (n.task.title, n.task.position))
        .collect();
    assert_eq!(
        children,
        vec![("A".into(), 1), ("B".into(), 2), ("C".into(), 3)]
    );
}

#[test]
fn positions_past_nine_sort_as_numbers_not_text() {
    let mut store = Store::open_in_memory().unwrap();
    for n in 1..=11 {
        store
            .add_task(titled(&format!("Task {n}")), &Actor::Me)
            .unwrap();
    }
    let positions: Vec<u32> = store
        .tree()
        .unwrap()
        .iter()
        .map(|n| n.task.position)
        .collect();
    assert_eq!(positions, (1..=11).collect::<Vec<u32>>());
}

#[test]
fn a_task_reads_back_with_everything_it_was_given() {
    let mut store = Store::open_in_memory().unwrap();
    let repo = store
        .add_repo(
            "web",
            std::path::Path::new("/w/web"),
            Some("git@github.com:o/web.git"),
            &Actor::Me,
        )
        .unwrap();
    let added = store
        .add_task(
            titled("Booking page")
                .with_body("Use the new API")
                .in_repo(repo.id),
            &Actor::Me,
        )
        .unwrap();

    let read = store.task(added.id).unwrap();
    assert_eq!(read, added);
    assert_eq!(read.body.as_deref(), Some("Use the new API"));
    assert_eq!(read.repo, Some(repo.id));
    assert_eq!(store.repos().unwrap(), vec![repo]);
}
