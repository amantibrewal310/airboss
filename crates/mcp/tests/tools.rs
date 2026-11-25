//! Each tool called in-process against an in-memory `Store`, without MCP.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use airboss_core::{Actor, LinkKind, NewLink, NewTask, RepoId, Status, Store, TaskId};
use airboss_mcp::identity::Identity;
use airboss_mcp::tools::{self, GetTask, ListTasks, StatusName};

const THREAD: &str = "97b5c8b5";

fn store() -> Store {
    Store::open_in_memory().unwrap()
}

/// A T3 thread running Claude Code, the usual case.
fn t3() -> Identity {
    Identity::from_env(
        |name| match name {
            "T3CODE_THREAD_ID" => Some(THREAD.into()),
            "CLAUDE_CODE_SESSION_ID" => Some("sess-1".into()),
            _ => None,
        },
        1,
    )
}

/// A thread that couldn't be identified.
fn unknown() -> Identity {
    Identity::from_env(|_| None, 77)
}

fn add(s: &mut Store, title: &str) -> TaskId {
    s.add_task(NewTask::new(title), &Actor::Me).unwrap().id
}

fn add_under(s: &mut Store, parent: TaskId, title: &str) -> TaskId {
    s.add_task(NewTask::new(title).under(parent), &Actor::Me)
        .unwrap()
        .id
}

fn repo(s: &mut Store, name: &str) -> RepoId {
    let path = format!("/src/{name}");
    s.add_repo(name, Path::new(&path), None, &Actor::Me)
        .unwrap()
        .id
}

fn set(s: &mut Store, id: TaskId, to: Status) {
    s.set_status(id, to, Some("because"), &Actor::Me).unwrap();
}

fn list(s: &Store, args: ListTasks) -> Vec<(i64, String)> {
    tools::list_tasks(s, &t3(), args)
        .unwrap()
        .tasks
        .into_iter()
        .map(|n| (n.id.0, n.title))
        .collect()
}

fn titles(s: &Store, args: ListTasks) -> Vec<String> {
    list(s, args).into_iter().map(|(_, t)| t).collect()
}

// ---- list_tasks ----

#[test]
fn list_tasks_leaves_out_done_and_dropped_by_default() {
    let mut s = store();
    for (title, to) in [
        ("todo", Status::Todo),
        ("doing", Status::Doing),
        ("review", Status::InReview),
        ("blocked", Status::Blocked),
        ("done", Status::Done),
        ("dropped", Status::Dropped),
    ] {
        let id = add(&mut s, title);
        set(&mut s, id, to);
    }
    let agent = Actor::Agent("t3/x".into());
    s.add_task(NewTask::new("proposed"), &agent).unwrap();
    assert_eq!(
        titles(&s, ListTasks::default()),
        ["todo", "doing", "review", "blocked", "proposed"]
    );
}

#[test]
fn list_tasks_status_picks_exactly_those_statuses() {
    let mut s = store();
    let a = add(&mut s, "a");
    add(&mut s, "b");
    let c = add(&mut s, "c");
    set(&mut s, a, Status::Done);
    set(&mut s, c, Status::Doing);
    let args = ListTasks {
        status: Some(vec![StatusName::Done, StatusName::Doing]),
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, args), ["a", "c"]);
}

#[test]
fn list_tasks_shows_tree_shape_waiting_and_repo_names() {
    let mut s = store();
    let web = repo(&mut s, "web");
    let api = repo(&mut s, "api");
    let top = s
        .add_task(NewTask::new("top").in_repo(web).in_repo(api), &Actor::Me)
        .unwrap()
        .id;
    let child = add_under(&mut s, top, "child");
    let other = add(&mut s, "other");
    s.add_dependency(child, other, &Actor::Me).unwrap();

    let got = tools::list_tasks(&s, &t3(), ListTasks::default()).unwrap();
    let node = |id: TaskId| got.tasks.iter().find(|n| n.id == id).unwrap();
    assert_eq!(node(top).repos, ["api", "web"]);
    assert_eq!((node(top).depth, node(top).parent), (0, None));
    assert_eq!((node(child).depth, node(child).parent), (1, Some(top)));
    assert!(node(child).waiting);
    assert!(!node(other).waiting);
    assert!(!got.truncated);
    assert_eq!(got.total, 3);
}

#[test]
fn list_tasks_repos_keeps_tasks_linked_to_any_of_them() {
    let mut s = store();
    let web = repo(&mut s, "web");
    let api = repo(&mut s, "api");
    repo(&mut s, "ios");
    s.add_task(NewTask::new("w").in_repo(web), &Actor::Me)
        .unwrap();
    s.add_task(NewTask::new("a").in_repo(api), &Actor::Me)
        .unwrap();
    add(&mut s, "none");
    let args = |names: &[&str]| ListTasks {
        repos: Some(names.iter().map(|n| n.to_string()).collect()),
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, args(&["web"])), ["w"]);
    assert_eq!(titles(&s, args(&["web", "api"])), ["w", "a"]);
    assert!(titles(&s, args(&["ios"])).is_empty());
}

#[test]
fn list_tasks_names_the_registered_repos_when_one_is_unknown() {
    let mut s = store();
    repo(&mut s, "web");
    repo(&mut s, "api");
    let args = ListTasks {
        repos: Some(vec!["wbe".into()]),
        ..ListTasks::default()
    };
    let err = tools::list_tasks(&s, &t3(), args).unwrap_err().to_string();
    assert_eq!(
        err,
        "There's no repo named \"wbe\". Registered repos: api, web."
    );
}

#[test]
fn list_tasks_says_when_no_repos_are_registered() {
    let s = store();
    let args = ListTasks {
        repos: Some(vec!["web".into()]),
        ..ListTasks::default()
    };
    let err = tools::list_tasks(&s, &t3(), args).unwrap_err().to_string();
    assert_eq!(
        err,
        "There's no repo named \"web\". No repos are registered yet; leave `repos` out."
    );
}

#[test]
fn list_tasks_mine_keeps_tasks_linked_to_this_thread() {
    let mut s = store();
    let a = add(&mut s, "mine via t3");
    let b = add(&mut s, "mine via claude");
    let c = add(&mut s, "someone else's");
    add(&mut s, "unlinked");
    let me = Actor::Agent("t3/x".into());
    s.link(a, NewLink::new(LinkKind::T3Thread, THREAD), &me)
        .unwrap();
    s.link(b, NewLink::new(LinkKind::ClaudeSession, "sess-1"), &me)
        .unwrap();
    s.link(c, NewLink::new(LinkKind::T3Thread, "other"), &me)
        .unwrap();
    let args = ListTasks {
        mine: true,
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, args), ["mine via t3", "mine via claude"]);
}

#[test]
fn list_tasks_mine_needs_a_known_thread() {
    let s = store();
    let args = ListTasks {
        mine: true,
        ..ListTasks::default()
    };
    let err = tools::list_tasks(&s, &unknown(), args)
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("This server doesn't know which thread it serves"),
        "{err}"
    );
}

#[test]
fn list_tasks_ready_keeps_todo_tasks_with_no_open_blockers() {
    let mut s = store();
    let free = add(&mut s, "free");
    let waits = add(&mut s, "waits");
    let doing = add(&mut s, "doing");
    let unblocked = add(&mut s, "unblocked");
    let finished = add(&mut s, "finished");
    s.add_dependency(waits, free, &Actor::Me).unwrap();
    s.add_dependency(unblocked, finished, &Actor::Me).unwrap();
    set(&mut s, doing, Status::Doing);
    set(&mut s, finished, Status::Done);
    let args = ListTasks {
        ready: true,
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, args), ["free", "unblocked"]);
}

#[test]
fn list_tasks_query_matches_titles_ignoring_case() {
    let mut s = store();
    add(&mut s, "Fix booking typo");
    add(&mut s, "Booking flow");
    add(&mut s, "Settings");
    let args = ListTasks {
        query: Some("BOOK".into()),
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, args), ["Fix booking typo", "Booking flow"]);
}

#[test]
fn list_tasks_filters_combine() {
    let mut s = store();
    let web = repo(&mut s, "web");
    let a = s
        .add_task(NewTask::new("web booking").in_repo(web), &Actor::Me)
        .unwrap()
        .id;
    s.add_task(NewTask::new("web other").in_repo(web), &Actor::Me)
        .unwrap();
    add(&mut s, "booking elsewhere");
    set(&mut s, a, Status::Doing);
    let args = ListTasks {
        repos: Some(vec!["web".into()]),
        query: Some("booking".into()),
        status: Some(vec![StatusName::Doing]),
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, args), ["web booking"]);
}

#[test]
fn list_tasks_stops_at_200_and_says_so() {
    let mut s = store();
    for i in 0..205 {
        add(&mut s, &format!("t{i}"));
    }
    let got = tools::list_tasks(&s, &t3(), ListTasks::default()).unwrap();
    assert_eq!(got.tasks.len(), 200);
    assert!(got.truncated);
    assert_eq!(got.total, 205);
}

// ---- get_task ----

#[test]
fn get_task_on_a_missing_id_names_it() {
    let s = store();
    let err = tools::get_task(&s, GetTask { id: 99 })
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "There's no task #99. List the tasks to find the right id."
    );
}

#[test]
fn get_task_returns_the_task_with_repos_branch_and_links() {
    let mut s = store();
    let web = repo(&mut s, "web");
    let id = s
        .add_task(
            NewTask::new("Booking").in_repo(web).with_body("notes"),
            &Actor::Me,
        )
        .unwrap()
        .id;
    s.set_branch(id, Some("booking-flow"), &Actor::Me).unwrap();
    s.link(
        id,
        NewLink::new(LinkKind::Doc, "/plans/booking.md").with_title("booking.md"),
        &Actor::Me,
    )
    .unwrap();

    let got = tools::get_task(&s, GetTask { id: id.0 }).unwrap();
    assert_eq!(got.task.title, "Booking");
    assert_eq!(got.task.repos, ["web"]);
    assert_eq!(got.task.branch.as_deref(), Some("booking-flow"));
    assert_eq!(got.task.body.as_deref(), Some("notes"));
    assert_eq!(got.links.len(), 1);
    assert_eq!(got.links[0].title.as_deref(), Some("booking.md"));
    assert_eq!(got.links[0].r#ref, "/plans/booking.md");
    assert!(got.subtasks.is_empty());
    assert!(got.run_order.is_none());
}

#[test]
fn get_task_keeps_the_last_20_history_entries_oldest_first() {
    let mut s = store();
    let id = add(&mut s, "busy");
    for i in 0..25 {
        s.add_note(id, &format!("note {i}"), &Actor::Me).unwrap();
    }
    let got = tools::get_task(&s, GetTask { id: id.0 }).unwrap();
    assert_eq!(got.history.len(), 20);
    assert_eq!(got.history[0].body.as_deref(), Some("note 5"));
    assert_eq!(got.history[19].body.as_deref(), Some("note 24"));
    assert_eq!(got.history_total, 26);
}

#[test]
fn get_task_lists_dependencies_both_ways_with_finished_ones() {
    let mut s = store();
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|t| add(&mut s, t));
    s.add_dependency(c, a, &Actor::Me).unwrap();
    s.add_dependency(c, b, &Actor::Me).unwrap();
    s.add_dependency(d, c, &Actor::Me).unwrap();
    set(&mut s, a, Status::Done);

    let got = tools::get_task(&s, GetTask { id: c.0 }).unwrap();
    let ids = |v: &[airboss_mcp::render::TaskRef]| v.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(ids(&got.dependencies.waits_on), [a, b]);
    assert_eq!(got.dependencies.waits_on[0].status, Status::Done);
    assert_eq!(ids(&got.dependencies.blocks), [d]);
    assert!(got.waiting);
}

#[test]
fn get_task_shows_subtasks_and_run_order() {
    let mut s = store();
    let parent = add(&mut s, "Plan");
    let one = add_under(&mut s, parent, "one");
    let two = add_under(&mut s, parent, "two");
    let three = add_under(&mut s, parent, "three");
    let outside = add(&mut s, "outside");
    s.add_dependency(two, one, &Actor::Me).unwrap();
    s.add_dependency(three, outside, &Actor::Me).unwrap();

    let got = tools::get_task(&s, GetTask { id: parent.0 }).unwrap();
    let sub: Vec<_> = got.subtasks.iter().map(|t| t.id).collect();
    assert_eq!(sub, [one, two, three]);
    let order = got.run_order.unwrap();
    let waves: Vec<Vec<TaskId>> = order
        .waves
        .iter()
        .map(|w| w.iter().map(|t| t.id).collect())
        .collect();
    assert_eq!(waves, [vec![one], vec![two]]);
    assert_eq!(order.waiting_outside.len(), 1);
    assert_eq!(order.waiting_outside[0].task.id, three);
    assert_eq!(order.waiting_outside[0].blockers, [outside]);
    assert!(order.tangled.is_empty());
}
