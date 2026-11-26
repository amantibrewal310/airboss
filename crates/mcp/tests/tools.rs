//! Each tool called in-process against an in-memory `Store`, without MCP.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use airboss_core::{Actor, LinkKind, NewLink, NewTask, RepoId, Status, Store, TaskId};
use airboss_mcp::identity::Identity;
use airboss_mcp::tools::{
    self, AgentStatus, DependsOn, GetTask, ImportPlan, Link, LinkKindName, ListTasks, PlanStep,
    PrStateName, Proposal, ProposeTasks, StatusName, UpdateTask,
};

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

// ---- update_task ----

fn update(
    s: &mut Store,
    id: TaskId,
    status: Option<AgentStatus>,
    note: Option<&str>,
) -> Result<airboss_mcp::render::TaskView, String> {
    tools::update_task(
        s,
        &t3(),
        UpdateTask {
            id: id.0,
            status,
            note: note.map(str::to_string),
        },
    )
    .map_err(|e| e.to_string())
}

fn last_event(s: &Store, id: TaskId) -> airboss_core::Event {
    s.history(id).unwrap().pop().unwrap()
}

#[test]
fn update_task_sets_the_status_as_this_agent() {
    let mut s = store();
    let id = add(&mut s, "work");
    let got = update(&mut s, id, Some(AgentStatus::Doing), None).unwrap();
    assert_eq!(got.status, Status::Doing);
    let e = last_event(&s, id);
    assert_eq!(e.actor, Actor::Agent(format!("t3/{THREAD}")));
    assert_eq!(e.to_status, Some(Status::Doing));
}

#[test]
fn update_task_with_only_a_note_adds_the_note() {
    let mut s = store();
    let id = add(&mut s, "work");
    let got = update(&mut s, id, None, Some("halfway there")).unwrap();
    assert_eq!(got.status, Status::Todo);
    assert_eq!(last_event(&s, id).body.as_deref(), Some("halfway there"));
}

#[test]
fn update_task_blocked_takes_the_note_as_its_reason() {
    let mut s = store();
    let id = add(&mut s, "work");
    let got = update(
        &mut s,
        id,
        Some(AgentStatus::Blocked),
        Some("needs an API key"),
    )
    .unwrap();
    assert_eq!(got.status, Status::Blocked);
    assert_eq!(got.blocked_reason.as_deref(), Some("needs an API key"));
}

#[test]
fn update_task_blocked_without_a_reason_asks_for_one() {
    let mut s = store();
    let id = add(&mut s, "work");
    let err = update(&mut s, id, Some(AgentStatus::Blocked), None).unwrap_err();
    assert_eq!(err, "Say why it's blocked in `note`.");
}

#[test]
fn update_task_done_without_a_note_asks_what_was_done() {
    let mut s = store();
    let id = add(&mut s, "work");
    let err = update(&mut s, id, Some(AgentStatus::Done), Some("  ")).unwrap_err();
    assert_eq!(err, "Say what you did in `note` when finishing a task.");
}

#[test]
fn update_task_done_with_a_note_finishes_a_task_without_a_pr() {
    let mut s = store();
    let id = add(&mut s, "work");
    let got = update(&mut s, id, Some(AgentStatus::Done), Some("Wrote the doc")).unwrap();
    assert_eq!(got.status, Status::Done);
}

#[test]
fn update_task_done_on_a_task_with_a_pr_says_to_use_in_review() {
    let mut s = store();
    let id = add(&mut s, "code");
    s.link(id, NewLink::pr("acme/web#42"), &Actor::Me).unwrap();
    let err = update(&mut s, id, Some(AgentStatus::Done), Some("merged it")).unwrap_err();
    assert_eq!(
        err,
        "Agents can't finish a task that has a PR; it finishes when the PR merges. Set it to in_review instead."
    );
}

#[test]
fn update_task_on_a_finished_task_names_its_status() {
    let mut s = store();
    let id = add(&mut s, "old");
    set(&mut s, id, Status::Done);
    let err = update(&mut s, id, Some(AgentStatus::Doing), None).unwrap_err();
    assert_eq!(
        err,
        format!(
            "Task #{} is done, so its status can't change from here. Ask the user to reopen it if there's more to do.",
            id.0
        )
    );
}

#[test]
fn update_task_on_a_proposal_says_to_wait_for_acceptance() {
    let mut s = store();
    let agent = Actor::Agent("t3/x".into());
    let id = s.add_task(NewTask::new("idea"), &agent).unwrap().id;
    let err = update(&mut s, id, Some(AgentStatus::Doing), None).unwrap_err();
    assert!(
        err.ends_with(
            "is proposed, so its status can't change from here. Wait for the user to accept it."
        ),
        "{err}"
    );
}

#[test]
fn update_task_needs_a_status_or_a_note() {
    let mut s = store();
    let id = add(&mut s, "work");
    assert_eq!(
        update(&mut s, id, None, None).unwrap_err(),
        "Give `status`, `note`, or both."
    );
    assert_eq!(
        update(&mut s, id, None, Some(" ")).unwrap_err(),
        "Give `status`, `note`, or both."
    );
}

#[test]
fn update_task_on_a_missing_task_names_it() {
    let mut s = store();
    let err = update(&mut s, TaskId(41), Some(AgentStatus::Doing), None).unwrap_err();
    assert_eq!(
        err,
        "There's no task #41. List the tasks to find the right id."
    );
    let err = update(&mut s, TaskId(41), None, Some("hi")).unwrap_err();
    assert_eq!(
        err,
        "There's no task #41. List the tasks to find the right id."
    );
}

// ---- link ----

fn link_args(id: TaskId, kind: LinkKindName) -> Link {
    Link {
        id: id.0,
        kind,
        r#ref: None,
        url: None,
        pr_state: None,
        title: None,
    }
}

#[test]
fn link_without_a_ref_links_this_t3_thread() {
    let mut s = store();
    let id = add(&mut s, "work");
    let got = tools::link(&mut s, &t3(), link_args(id, LinkKindName::T3Thread)).unwrap();
    assert_eq!((got.kind, got.r#ref.as_str()), (LinkKind::T3Thread, THREAD));
    assert_eq!(got.linked_by, Actor::Agent(format!("t3/{THREAD}")));
    // Now it's this thread's task.
    let mine = ListTasks {
        mine: true,
        ..ListTasks::default()
    };
    assert_eq!(titles(&s, mine), ["work"]);
}

#[test]
fn link_without_a_ref_links_this_claude_session() {
    let mut s = store();
    let id = add(&mut s, "work");
    let got = tools::link(&mut s, &t3(), link_args(id, LinkKindName::ClaudeSession)).unwrap();
    assert_eq!(got.r#ref, "sess-1");
}

#[test]
fn link_without_a_ref_needs_a_known_thread() {
    let mut s = store();
    let id = add(&mut s, "work");
    let err = tools::link(&mut s, &unknown(), link_args(id, LinkKindName::T3Thread))
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "This server doesn't know its T3 thread id, so give `ref`."
    );
}

#[test]
fn link_a_pr_with_title_and_url_starts_it_open() {
    let mut s = store();
    let id = add(&mut s, "code");
    let args = Link {
        r#ref: Some("acme/web#42".into()),
        url: Some("https://github.com/acme/web/pull/42".into()),
        title: Some("Fix typo".into()),
        ..link_args(id, LinkKindName::Pr)
    };
    let got = tools::link(&mut s, &t3(), args).unwrap();
    assert_eq!(got.pr_state, Some(airboss_core::PrState::Open));
    assert_eq!(got.title.as_deref(), Some("Fix typo"));
    assert_eq!(
        got.url.as_deref(),
        Some("https://github.com/acme/web/pull/42")
    );
}

#[test]
fn link_takes_a_known_pr_state() {
    let mut s = store();
    let id = add(&mut s, "code");
    let args = Link {
        r#ref: Some("acme/web#7".into()),
        pr_state: Some(PrStateName::Merged),
        ..link_args(id, LinkKindName::Pr)
    };
    let got = tools::link(&mut s, &t3(), args).unwrap();
    assert_eq!(got.pr_state, Some(airboss_core::PrState::Merged));
}

#[test]
fn link_refuses_a_pr_state_on_anything_but_a_pr() {
    let mut s = store();
    let id = add(&mut s, "doc");
    let args = Link {
        r#ref: Some("/notes.md".into()),
        pr_state: Some(PrStateName::Open),
        ..link_args(id, LinkKindName::Doc)
    };
    let err = tools::link(&mut s, &t3(), args).unwrap_err().to_string();
    assert_eq!(err, "`pr_state` only goes with a `pr` link; leave it out.");
}

#[test]
fn link_needs_a_ref_for_anything_but_this_thread() {
    let mut s = store();
    let id = add(&mut s, "code");
    let err = tools::link(&mut s, &t3(), link_args(id, LinkKindName::Pr))
        .unwrap_err()
        .to_string();
    assert_eq!(err, "Give `ref` for a pr link, e.g. owner/repo#42.");
}

#[test]
fn link_twice_returns_the_same_link() {
    let mut s = store();
    let id = add(&mut s, "work");
    let a = tools::link(&mut s, &t3(), link_args(id, LinkKindName::T3Thread)).unwrap();
    let b = tools::link(&mut s, &t3(), link_args(id, LinkKindName::T3Thread)).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(s.links(id).unwrap().len(), 1);
}

#[test]
fn link_on_a_missing_task_names_it() {
    let mut s = store();
    let err = tools::link(&mut s, &t3(), link_args(TaskId(5), LinkKindName::T3Thread))
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "There's no task #5. List the tasks to find the right id."
    );
}

// ---- propose_tasks ----

fn proposal(title: &str) -> Proposal {
    Proposal {
        title: title.into(),
        body: None,
        repos: vec![],
    }
}

fn propose(
    s: &mut Store,
    parent: Option<TaskId>,
    tasks: Vec<Proposal>,
) -> Result<Vec<airboss_mcp::render::TaskView>, String> {
    tools::propose_tasks(
        s,
        &t3(),
        ProposeTasks {
            parent: parent.map(|p| p.0),
            tasks,
        },
    )
    .map_err(|e| e.to_string())
}

fn task_count(s: &Store) -> usize {
    s.tree().unwrap().len()
}

#[test]
fn propose_tasks_always_makes_proposals() {
    let mut s = store();
    let web = repo(&mut s, "web");
    let parent = add(&mut s, "Booking");
    let got = propose(
        &mut s,
        Some(parent),
        vec![
            Proposal {
                body: Some("why".into()),
                repos: vec!["web".into()],
                ..proposal("Add tests")
            },
            proposal("Update docs"),
        ],
    )
    .unwrap();
    assert_eq!(got.len(), 2);
    for t in &got {
        assert_eq!(t.status, Status::Proposed);
        assert_eq!(t.origin, airboss_core::Origin::Agent);
        assert_eq!(t.parent, Some(parent));
    }
    assert_eq!(got[0].repos, ["web"]);
    assert_eq!(got[0].body.as_deref(), Some("why"));
    assert_eq!(s.task(got[0].id).unwrap().repos, [web]);
    let e = last_event(&s, got[1].id);
    assert_eq!(e.actor, Actor::Agent(format!("t3/{THREAD}")));
}

#[test]
fn propose_tasks_takes_between_1_and_20() {
    let mut s = store();
    assert_eq!(
        propose(&mut s, None, vec![]).unwrap_err(),
        "Propose between 1 and 20 tasks at a time."
    );
    let many = (0..21).map(|i| proposal(&format!("t{i}"))).collect();
    assert_eq!(
        propose(&mut s, None, many).unwrap_err(),
        "Propose between 1 and 20 tasks at a time."
    );
    assert_eq!(task_count(&s), 0);
}

#[test]
fn propose_tasks_checks_everything_before_writing() {
    let mut s = store();
    repo(&mut s, "web");
    let err = propose(&mut s, None, vec![proposal("fine"), proposal("  ")]).unwrap_err();
    assert_eq!(err, "Give every task a title that isn't blank.");
    let bad_repo = Proposal {
        repos: vec!["nope".into()],
        ..proposal("second")
    };
    let err = propose(&mut s, None, vec![proposal("fine"), bad_repo]).unwrap_err();
    assert_eq!(
        err,
        "There's no repo named \"nope\". Registered repos: web."
    );
    assert_eq!(task_count(&s), 0);
}

#[test]
fn propose_tasks_under_a_finished_parent_says_so() {
    let mut s = store();
    let parent = add(&mut s, "old");
    set(&mut s, parent, Status::Done);
    let err = propose(&mut s, Some(parent), vec![proposal("more")]).unwrap_err();
    assert_eq!(
        err,
        format!(
            "Task #{} is done, so nothing can go under it. Pick an open parent or none.",
            parent.0
        )
    );
    assert_eq!(task_count(&s), 1);
}

#[test]
fn propose_tasks_under_a_missing_parent_names_it() {
    let mut s = store();
    let err = propose(&mut s, Some(TaskId(8)), vec![proposal("more")]).unwrap_err();
    assert_eq!(
        err,
        "There's no task #8. List the tasks to find the right id."
    );
}

// ---- import_plan ----

struct PlanFile(std::path::PathBuf);

impl PlanFile {
    fn new(name: &str) -> PlanFile {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "airboss-mcp-plan-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, "# plan").unwrap();
        PlanFile(path.canonicalize().unwrap())
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for PlanFile {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn step(key: &str, title: &str) -> PlanStep {
    PlanStep {
        key: key.into(),
        title: title.into(),
        body: None,
        repos: vec![],
        depends_on: vec![],
        children: vec![],
    }
}

fn booking_plan() -> Vec<PlanStep> {
    vec![
        PlanStep {
            children: vec![step("p1.a", "Schema"), step("p1.b", "API")],
            ..step("p1", "Phase 1")
        },
        PlanStep {
            depends_on: vec![DependsOn::Key("p1".into())],
            repos: vec!["web".into()],
            ..step("p2", "Phase 2")
        },
    ]
}

fn import(
    s: &mut Store,
    file: &str,
    parent: Option<TaskId>,
    plan: Vec<PlanStep>,
) -> Result<airboss_mcp::render::PlanView, String> {
    tools::import_plan(
        s,
        &t3(),
        ImportPlan {
            plan_file: file.into(),
            parent: parent.map(|p| p.0),
            plan,
        },
    )
    .map_err(|e| e.to_string())
}

#[test]
fn import_plan_creates_the_tree_as_plan() {
    let mut s = store();
    repo(&mut s, "web");
    let file = PlanFile::new("booking.md");
    let got = import(&mut s, &file.path(), None, booking_plan()).unwrap();
    assert_eq!(got.created.len(), 4);
    assert!(got.existing.is_empty());
    let p1 = got.keys["p1"];
    let p2 = got.keys["p2"];
    let schema = s.task(got.keys["p1.a"]).unwrap();
    assert_eq!(schema.parent, Some(p1));
    assert_eq!(schema.origin, airboss_core::Origin::Plan);
    assert_eq!(schema.status, Status::Todo);
    let deps = s.dependencies(p2).unwrap();
    assert_eq!(deps.waits_on.iter().map(|t| t.id).collect::<Vec<_>>(), [p1]);
    assert_eq!(s.task(p2).unwrap().repos.len(), 1);
}

#[test]
fn import_plan_links_the_plan_file_to_its_top_level_tasks() {
    let mut s = store();
    repo(&mut s, "web");
    let file = PlanFile::new("booking.md");
    let got = import(&mut s, &file.path(), None, booking_plan()).unwrap();
    for key in ["p1", "p2"] {
        let links = s.links(got.keys[key]).unwrap();
        assert_eq!(links.len(), 1, "{key}");
        assert_eq!(links[0].kind, LinkKind::Doc);
        assert_eq!(links[0].r#ref, file.path());
        assert_eq!(links[0].title.as_deref(), Some("booking.md"));
        assert_eq!(links[0].linked_by, Actor::Plan);
    }
    assert!(s.links(got.keys["p1.a"]).unwrap().is_empty());
}

#[test]
fn import_plan_records_which_thread_sent_it_on_every_created_task() {
    let mut s = store();
    repo(&mut s, "web");
    let file = PlanFile::new("booking.md");
    let got = import(&mut s, &file.path(), None, booking_plan()).unwrap();
    for id in &got.created {
        let history = s.history(*id).unwrap();
        assert_eq!(history[0].actor, Actor::Plan);
        let note = history
            .iter()
            .find(|e| e.kind == airboss_core::EventKind::Note)
            .expect("an import note");
        assert_eq!(note.actor, Actor::Agent(format!("t3/{THREAD}")));
        assert_eq!(note.body.as_deref(), Some("Imported from booking.md"));
    }
}

#[test]
fn import_plan_twice_changes_nothing() {
    let mut s = store();
    repo(&mut s, "web");
    let file = PlanFile::new("booking.md");
    let first = import(&mut s, &file.path(), None, booking_plan()).unwrap();
    let events = s.events_after(airboss_core::EventId(0)).unwrap().len();
    let again = import(&mut s, &file.path(), None, booking_plan()).unwrap();
    assert!(again.created.is_empty());
    assert_eq!(again.existing.len(), 4);
    assert_eq!(again.keys, first.keys);
    assert_eq!(
        s.events_after(airboss_core::EventId(0)).unwrap().len(),
        events
    );
}

#[test]
fn import_plan_refuses_a_plan_file_that_does_not_exist() {
    let mut s = store();
    let err = import(&mut s, "/no/such/plan.md", None, vec![step("a", "A")]).unwrap_err();
    assert_eq!(
        err,
        "There's no file at \"/no/such/plan.md\". Give the path of the plan file the user settled in plan-review."
    );
    assert_eq!(task_count(&s), 0);
}

#[test]
fn import_plan_needs_a_step() {
    let mut s = store();
    let file = PlanFile::new("p.md");
    let err = import(&mut s, &file.path(), None, vec![]).unwrap_err();
    assert_eq!(err, "Give `plan` at least one step.");
}

#[test]
fn import_plan_can_wait_on_an_existing_task_and_go_under_a_parent() {
    let mut s = store();
    let parent = add(&mut s, "Booking");
    let before = add(&mut s, "Design");
    let file = PlanFile::new("p.md");
    let plan = vec![PlanStep {
        depends_on: vec![DependsOn::Task(before.0)],
        ..step("build", "Build")
    }];
    let got = import(&mut s, &file.path(), Some(parent), plan).unwrap();
    let build = got.keys["build"];
    assert_eq!(s.task(build).unwrap().parent, Some(parent));
    assert_eq!(s.dependencies(build).unwrap().waits_on[0].id, before);
}

#[test]
fn import_plan_explains_duplicate_keys_loops_and_bad_targets() {
    let mut s = store();
    let file = PlanFile::new("p.md");
    let err = import(
        &mut s,
        &file.path(),
        None,
        vec![step("a", "A"), step("a", "B")],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "The plan uses the key \"a\" twice. Give every step its own key."
    );

    let looped = vec![
        PlanStep {
            depends_on: vec![DependsOn::Key("b".into())],
            ..step("a", "A")
        },
        PlanStep {
            depends_on: vec![DependsOn::Key("a".into())],
            ..step("b", "B")
        },
    ];
    let err = import(&mut s, &file.path(), None, looped).unwrap_err();
    assert_eq!(
        err,
        "That would make a loop of tasks waiting on each other. Remove one of the dependencies."
    );

    let on_parent = vec![PlanStep {
        children: vec![PlanStep {
            depends_on: vec![DependsOn::Key("top".into())],
            ..step("kid", "Kid")
        }],
        ..step("top", "Top")
    }];
    let err = import(&mut s, &file.path(), None, on_parent).unwrap_err();
    assert_eq!(
        err,
        "A task can't wait on its own parent or subtask. Remove that dependency."
    );

    let done = add(&mut s, "done");
    set(&mut s, done, Status::Done);
    let err = import(&mut s, &file.path(), Some(done), vec![step("x", "X")]).unwrap_err();
    assert_eq!(
        err,
        format!(
            "Task #{} is done, so nothing can go under it. Pick an open parent or none.",
            done.0
        )
    );

    let err = import(
        &mut s,
        &file.path(),
        None,
        vec![PlanStep {
            depends_on: vec![DependsOn::Task(999)],
            ..step("y", "Y")
        }],
    )
    .unwrap_err();
    assert_eq!(
        err,
        "There's no task #999. List the tasks to find the right id."
    );
    assert_eq!(task_count(&s), 1);
}

#[test]
fn import_plan_names_an_unknown_repo() {
    let mut s = store();
    let file = PlanFile::new("p.md");
    let plan = vec![PlanStep {
        repos: vec!["web".into()],
        ..step("a", "A")
    }];
    let err = import(&mut s, &file.path(), None, plan).unwrap_err();
    assert_eq!(
        err,
        "There's no repo named \"web\". No repos are registered yet; leave `repos` out."
    );
}

#[test]
fn link_with_a_malformed_pr_ref_says_what_a_pr_ref_looks_like() {
    let mut s = store();
    let id = add(&mut s, "work");
    let args = Link {
        r#ref: Some("acme/web/42".into()),
        ..link_args(id, LinkKindName::Pr)
    };
    let err = tools::link(&mut s, &t3(), args).unwrap_err().to_string();
    assert!(err.contains("owner/repo#42"), "{err}");
    assert!(
        err.contains("https://github.com/owner/repo/pull/42"),
        "{err}"
    );
}

#[test]
fn link_takes_a_pr_url_and_reports_the_canonical_ref() {
    let mut s = store();
    let id = add(&mut s, "work");
    let args = Link {
        r#ref: Some("https://github.com/Acme/Web/pull/42/files".into()),
        ..link_args(id, LinkKindName::Pr)
    };
    let got = tools::link(&mut s, &t3(), args).unwrap();
    assert_eq!(got.r#ref, "acme/web#42");
}
