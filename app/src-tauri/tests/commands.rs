//! Every command in Appendix A, called as a plain function against an in-memory store and
//! fake services that check the store lock is free whenever they're called.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use airboss_app::commands::app as appcmd;
use airboss_app::commands::launch;
use airboss_app::commands::tasks::{self, Direction};
use airboss_app::commands::{detail, needs, repos};
use airboss_app::dto::{
    CopyWhat, CreateTaskInput, DoItPlace, DoItResult, NewLinkText, OpenTarget, Then, ToastArgs,
    ToastTone,
};
use airboss_app::errors::{AppError, ErrorCode};
use airboss_app::menu::{FocusContext, WindowName, Zone};
use airboss_app::settings::{Appearance, Settings};
use airboss_app::{Caller, UiEvent};
use airboss_core::{Actor, LinkKind, NewLink, PrState, RepoId, Status, TaskId};
use airboss_services::{
    AttentionService, Availability, Checks, Chip, Connection, ErrorCode as SvcCode, LaunchReport,
    Mode, NeedsKind, Place, PrRef, PrView, Prepared, Review, ServiceError, Source, StartedThread,
    SyncSnapshot, T3Project, ThreadKey, ThreadState, ThreadView,
};
use common::{TestApp, needs_item, temp_dir};

const M: Caller = Caller::Main;

fn focus(zone: Zone) -> FocusContext {
    FocusContext {
        window: WindowName::Main,
        zone,
        selection: Some(TaskId(2)),
        view: "all".into(),
    }
}

fn input(title: &str) -> CreateTaskInput {
    CreateTaskInput {
        title: title.into(),
        body: None,
        parent: None,
        repos: vec![],
        links: vec![],
        then: Then::None,
    }
}

fn err_code<T: std::fmt::Debug>(r: Result<T, AppError>) -> (ErrorCode, String) {
    let e = r.unwrap_err();
    (e.code, e.message)
}

fn repo(app: &TestApp, name: &str) -> RepoId {
    app.core(|s| {
        s.add_repo(name, Path::new(&format!("/src/{name}")), None, &Actor::Me)
            .unwrap()
            .id
    })
}

/// Every write: the services heard about it with the lock free.
fn assert_told(app: &TestApp) {
    app.assert_lock_free();
    assert!(
        app.attention
            .calls()
            .iter()
            .any(|c| c.starts_with("store_changed")),
        "{:?}",
        app.attention.calls()
    );
}

// ---------------------------------------------------------------------------- reads

#[test]
fn bootstrap_returns_rows_in_tree_order_and_everything_the_window_draws_first() {
    let app = TestApp::new();
    let empty = detail::bootstrap(&app.state, M).unwrap();
    assert!(empty.rows.is_empty());
    let web = repo(&app, "web");
    let p = app.add("Parent", None);
    let c = app.add("Child", Some(p));
    app.attention
        .set_items(vec![needs_item("q", NeedsKind::Question, c, common::NOW)]);
    app.sync_vm();
    let b = detail::bootstrap(&app.state, M).unwrap();
    assert_eq!(b.order, vec![p, c]);
    assert_eq!(b.rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![p, c]);
    assert_eq!(b.repos[0].id, web);
    assert_eq!(b.needs.count, 1);
    assert_eq!(b.settings, Settings::default());
    assert!(!b.cli.installed);
    assert!(b.cli.path.ends_with(".local/bin/airboss"));
    assert!(b.connections.iter().any(|c| c.name == "Agents" && !c.ok));
    assert!(b.connections.iter().any(|c| c.name == "T3 Code" && c.ok));
    assert!(b.seq >= 1);
    assert_eq!(b.rows[0].last_actor, "me");
    app.assert_lock_free();
}

fn view(id: &str, state: ThreadState) -> ThreadView {
    ThreadView {
        key: ThreadKey {
            source: Source::T3,
            r#ref: id.into(),
        },
        linked: true,
        state,
        title: Some(format!("{id} title")),
        request: None,
        error: None,
        run_id: None,
        project_path: None,
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: None,
        last_reply: None,
        stale: false,
    }
}

fn pr_view(r: &str, state: PrState, head: Option<&str>, linked: bool) -> PrView {
    PrView {
        r#ref: PrRef::parse(r).unwrap(),
        linked,
        url: Some(format!("https://github.com/{}", r.replace('#', "/pull/"))),
        title: Some(format!("PR {r}")),
        state: Some(state),
        draft: false,
        review: Review::Approved,
        checks: Checks::Passing,
        additions: Some(412),
        deletions: Some(38),
        head_ref: head.map(str::to_string),
        author: None,
        updated_at: None,
        ready_since: None,
        problem: None,
        stale: false,
    }
}

#[test]
fn task_detail_gathers_links_threads_history_deps_needs_and_prepare() {
    let app = TestApp::new();
    let p = app.add("Booking flow", None);
    // Before it had steps: a parent takes no branch (SPEC-core rule 10).
    app.core(|s| s.set_branch(p, Some("task-1-booking"), &Actor::Me).unwrap());
    let t = app.add("Web: slot picker", Some(p));
    let blocker = app.add("API", Some(p));
    let after = app.add("Confirm", Some(p));
    app.core(|s| {
        s.add_dependency(t, blocker, &Actor::Me).unwrap();
        s.add_dependency(after, t, &Actor::Me).unwrap();
        s.link(t, NewLink::pr("acme/web#58"), &Actor::Me).unwrap();
        s.link(
            t,
            NewLink::new(LinkKind::T3Thread, "slot-picker"),
            &Actor::Me,
        )
        .unwrap();
        s.add_note(t, "from the agent", &Actor::Agent("t3/slot-picker".into()))
            .unwrap();
    });
    let mut snap = SyncSnapshot::default();
    let th = view("slot-picker", ThreadState::Running);
    snap.thread_tasks.insert(th.key.clone(), vec![t]);
    snap.threads.insert(th.key.clone(), th);
    let pr = pr_view("acme/web#58", PrState::Open, None, true);
    snap.pr_tasks.insert(pr.r#ref.clone(), vec![t]);
    snap.prs.insert(pr.r#ref.clone(), pr);
    let mine = pr_view("acme/web#60", PrState::Open, Some("task-1-booking"), false);
    snap.prs.insert(mine.r#ref.clone(), mine);
    app.sync.set_snapshot(snap);
    app.attention
        .set_items(vec![needs_item("q", NeedsKind::Question, t, common::NOW)]);
    app.sync_vm();

    let d = detail::task_detail(&app.state, M, t, 9, 30).unwrap();
    assert_eq!(d.req, 9);
    assert_eq!(d.task.title, "Web: slot picker");
    assert_eq!(d.links.len(), 2);
    let pr_link = d.links.iter().find(|l| l.r#ref == "acme/web#58").unwrap();
    assert_eq!(pr_link.pr.as_ref().unwrap().additions, Some(412));
    assert_eq!(pr_link.linked_by, "me");
    let th_link = d.links.iter().find(|l| l.r#ref == "slot-picker").unwrap();
    let thread = th_link.thread.as_ref().unwrap();
    assert_eq!(thread.chip, Some(Chip::Working));
    assert_eq!(
        thread.url.as_deref(),
        Some("t3code://threads/fake-environment/slot-picker")
    );
    assert_eq!(
        d.waits_on.iter().map(|x| x.id).collect::<Vec<_>>(),
        vec![blocker]
    );
    assert_eq!(
        d.blocks.iter().map(|x| x.id).collect::<Vec<_>>(),
        vec![after]
    );
    assert!(d.blocks[0].waiting);
    assert!(d.waves.is_none(), "a leaf has no waves");
    assert_eq!(d.needs.len(), 1);
    assert!(d.prepared.is_some());
    assert_eq!(
        d.suggested_prs.len(),
        1,
        "my open PR from the inherited branch"
    );
    // Newest first; the agent is named by its thread.
    assert_eq!(d.history[0].actor, "agent:t3/slot-picker");
    assert_eq!(
        d.history[0].thread_title.as_deref(),
        Some("slot-picker title")
    );
    assert!(!d.history_more);
    assert!(
        app.sync.calls().contains(&format!("refresh(PrsOf({t:?}))")),
        "an open PR asks sync to refresh: {:?}",
        app.sync.calls()
    );
    // A parent has waves; the history includes its subtasks'.
    let dp = detail::task_detail(&app.state, M, p, 1, 2).unwrap();
    assert!(dp.waves.is_some());
    assert!(dp.history_more);
    app.assert_lock_free();
}

#[test]
fn task_detail_skips_prepare_for_closed_tasks_and_says_a_gone_task_is_gone() {
    let app = TestApp::new();
    let t = app.add("Done", None);
    app.core(|s| s.set_status(t, Status::Done, None, &Actor::Me).unwrap());
    app.sync_vm();
    let d = detail::task_detail(&app.state, M, t, 1, 30).unwrap();
    assert!(d.prepared.is_none());
    assert!(!app.launch.calls().iter().any(|c| c.starts_with("prepare")));
    assert_eq!(
        err_code(detail::task_detail(&app.state, M, TaskId(999), 1, 30)),
        (ErrorCode::NotFound, "Task #999 no longer exists.".into())
    );
    // T3 missing: no prepared, and no error either (the footer and availability say it).
    let open = app.add("Open", None);
    app.launch.set_prepare(Err(ServiceError::new(
        SvcCode::T3Missing,
        "T3 Code isn't installed.",
    )));
    let d = detail::task_detail(&app.state, M, open, 1, 30).unwrap();
    assert!(d.prepared.is_none());
}

#[test]
fn task_detail_lists_the_proposals_under_it_with_who_made_them_and_their_edges() {
    let app = TestApp::new();
    let p = app.add("Plan", None);
    let phase = app.add("Phase", Some(p));
    let step = app.add("Step", Some(phase));
    let propose = |title: &str, under: TaskId, who: &str| {
        app.core(|s| {
            s.add_task(
                airboss_core::NewTask::new(title).under(under),
                &Actor::Agent(who.into()),
            )
            .unwrap()
            .id
        })
    };
    let a = propose("A", p, "t3/one");
    let b = propose("B", phase, "t3/two");
    app.core(|s| {
        s.add_dependency(a, step, &Actor::Me).unwrap();
        s.add_dependency(step, b, &Actor::Me).unwrap();
    });
    app.sync_vm();

    let d = detail::task_detail(&app.state, M, p, 1, 30).unwrap();
    let got: Vec<_> = d
        .proposals
        .iter()
        .map(|x| {
            (
                x.id,
                x.parent,
                x.by.as_str(),
                x.follows.clone(),
                x.blocks.clone(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (a, p, "agent:t3/one", vec![step], vec![]),
            (b, phase, "agent:t3/two", vec![], vec![step]),
        ]
    );
    assert!(d.proposals.iter().all(|x| !x.at.is_empty()));
    // A leaf has none; neither does a parent once they're accepted.
    assert!(
        detail::task_detail(&app.state, M, step, 1, 30)
            .unwrap()
            .proposals
            .is_empty()
    );
    tasks::accept_proposals(&app.state, M, vec![a, b]).unwrap();
    assert!(
        detail::task_detail(&app.state, M, p, 1, 30)
            .unwrap()
            .proposals
            .is_empty()
    );
    app.assert_lock_free();
}

#[test]
fn run_map_plans_and_quickadd_data_read_the_view_model() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    let p = app.add("Plan", None);
    let a = app.add("A", Some(p));
    let b = app.add("B", Some(p));
    let done = app.add("Done", None);
    app.core(|s| s.set_status(done, Status::Done, None, &Actor::Me).unwrap());
    app.sync_vm();
    let map = detail::run_map(&app.state, M, p, 4).unwrap();
    assert_eq!((map.nodes.len(), map.waves.len()), (2, 1));
    let cards = detail::plans(&app.state, M, 5).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(
        cards[0].next_up.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![a, b]
    );
    let q = detail::quickadd_data(&app.state, M).unwrap();
    assert_eq!(q.repos[0].id, web);
    assert_eq!(
        q.parents.iter().map(|p| p.id).collect::<Vec<_>>(),
        vec![p, a, b]
    );
    assert_eq!(
        err_code(detail::run_map(&app.state, M, TaskId(77), 1)).0,
        ErrorCode::NotFound
    );
}

#[test]
fn t3_projects_says_which_are_git_repos_and_which_are_added() {
    let app = TestApp::new();
    let dir = temp_dir("t3");
    let git = dir.join("booking-web");
    std::fs::create_dir_all(git.join(".git")).unwrap();
    let plain = dir.join("notes");
    std::fs::create_dir_all(&plain).unwrap();
    app.core(|s| s.add_repo("booking-web", &git, None, &Actor::Me).unwrap());
    app.sync_vm();
    app.sync.set_snapshot(SyncSnapshot {
        projects: vec![
            T3Project {
                id: "1".into(),
                name: "booking-web".into(),
                path: git.clone(),
            },
            T3Project {
                id: "2".into(),
                name: "notes".into(),
                path: plain.clone(),
            },
        ],
        ..SyncSnapshot::default()
    });
    let list = detail::t3_projects(&app.state, M).unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[0].is_git && list[0].added);
    assert!(!list[1].is_git && !list[1].added);
}

// ---------------------------------------------------------------------------- task writes

#[test]
fn create_task_links_repos_in_the_same_change_then_each_link() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    let parent = app.add("Parent", None);
    app.sync_vm();
    let out = tasks::create_task(
        &app.state,
        M,
        CreateTaskInput {
            title: "Slot picker".into(),
            body: Some("Notes".into()),
            parent: Some(parent),
            repos: vec![web],
            links: vec![
                NewLinkText {
                    text: "https://acme.slack.com/archives/C1/p2".into(),
                    title: Some("#booking thread".into()),
                },
                NewLinkText {
                    text: "not a link".into(),
                    title: None,
                },
            ],
            then: Then::None,
        },
    )
    .unwrap();
    let task = app.core(|s| s.task(out.id).unwrap());
    assert_eq!(
        (task.parent, task.repos.clone(), task.body.as_deref()),
        (Some(parent), vec![web], Some("Notes"))
    );
    let links = app.core(|s| s.links(out.id).unwrap());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].title.as_deref(), Some("#booking thread"));
    assert_eq!(out.link_errors.len(), 1);
    assert_eq!(out.link_errors[0].text, "not a link");
    assert_eq!(out.link_errors[0].error.code, ErrorCode::NotALink);
    assert!(out.patch.upserts.iter().any(|r| r.id == out.id));
    assert!(out.patch.created.contains(&out.id));
    assert!(out.launch.is_none());
    assert_told(&app);
}

#[test]
fn create_task_from_quick_add_shows_the_main_window_when_a_link_fails() {
    let app = TestApp::new();
    let mut task = input("T");
    task.links = vec![NewLinkText {
        text: "nope".into(),
        title: None,
    }];
    let out = tasks::create_task(&app.state, Caller::QuickAdd, task).unwrap();
    assert!(app.shell.calls().contains(&"show_main".to_string()));
    let toast = app.shell.events().into_iter().find_map(|e| match e {
        UiEvent::Toast(t) => Some(t),
        _ => None,
    });
    assert_eq!(
        toast.unwrap().text,
        format!("Created #{}, but couldn't link nope", out.id.0)
    );
}

#[test]
fn create_task_then_do_it_starts_it_or_asks_where() {
    let app = TestApp::new();
    let mut task = input("Go");
    task.then = Then::DoIt;
    let out = tasks::create_task(&app.state, M, task).unwrap();
    assert!(matches!(out.launch, Some(DoItResult::Started { .. })));
    assert_eq!(app.launch.starts()[0].task, out.id);

    app.launch.set_prepare(Ok(Prepared {
        needs_choice: true,
        ..app_prepared()
    }));
    let mut task = input("Where");
    task.then = Then::DoIt;
    let out = tasks::create_task(&app.state, M, task).unwrap();
    assert!(matches!(out.launch, Some(DoItResult::NeedsChoice { .. })));
    let mut task = input("Plan");
    task.then = Then::PlanIt;
    let out = tasks::create_task(&app.state, M, task).unwrap();
    assert!(out.launch.is_some());
    assert_eq!(app.launch.starts().last().unwrap().mode, Mode::PlanIt);

    // A launch that fails leaves the task and says so in a toast.
    app.launch.set_start(Err(ServiceError::new(
        SvcCode::T3NotRunning,
        "Open T3 Code.",
    )));
    let mut task = input("Fails");
    task.then = Then::PlanIt;
    let out = tasks::create_task(&app.state, M, task).unwrap();
    assert!(out.launch.is_none());
    assert!(app.state.row(out.id).is_some());
    assert!(
        app.shell
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::Toast(t) if t.text.contains("Open T3 Code.")))
    );
}

#[test]
fn create_task_refuses_an_empty_title_and_a_closed_parent() {
    let app = TestApp::new();
    assert_eq!(
        err_code(tasks::create_task(&app.state, M, input("  "))),
        (ErrorCode::EmptyTitle, "A task needs a title.".into())
    );
    let done = app.add("Done", None);
    app.core(|s| s.set_status(done, Status::Done, None, &Actor::Me).unwrap());
    let mut task = input("x");
    task.parent = Some(done);
    assert_eq!(
        err_code(tasks::create_task(&app.state, M, task)),
        (
            ErrorCode::InvalidTarget,
            "Tasks can't go under a proposed, done or dropped task.".into()
        )
    );
}

#[test]
fn rename_notes_note_and_branch_return_their_patches() {
    let app = TestApp::new();
    let t = app.add("Old", None);
    app.sync_vm();
    let p = tasks::rename_task(&app.state, M, t, "New".into()).unwrap();
    assert_eq!(p.upserts[0].title, "New");
    assert_told(&app);
    let p = tasks::edit_notes(&app.state, M, t, Some("Body".into())).unwrap();
    assert!(p.seq > 0);
    assert_eq!(app.core(|s| s.task(t).unwrap().body), Some("Body".into()));
    tasks::edit_notes(&app.state, M, t, Some("  ".into())).unwrap();
    assert_eq!(app.core(|s| s.task(t).unwrap().body), None);
    tasks::add_note(&app.state, M, t, "for the agent".into()).unwrap();
    let p = tasks::set_branch(&app.state, M, t, Some("task-1-new".into())).unwrap();
    assert_eq!(p.upserts[0].branch.as_deref(), Some("task-1-new"));
    assert_eq!(
        err_code(tasks::set_branch(
            &app.state,
            M,
            t,
            Some("bad branch".into())
        )),
        (
            ErrorCode::InvalidTarget,
            "Branch names can use letters, digits, `.`, `_`, `/` and `-`.".into()
        )
    );
    assert_eq!(
        err_code(tasks::rename_task(&app.state, M, TaskId(404), "x".into())).1,
        "Task #404 no longer exists."
    );
}

#[test]
fn set_status_and_toggle_done_return_a_patch_and_an_undo_entry() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    let r = tasks::set_status(&app.state, M, t, Status::Doing, None).unwrap();
    assert_eq!(r.patch.as_ref().unwrap().upserts[0].status, Status::Doing);
    assert_eq!(
        r.undo.unwrap().label,
        format!("Set #{} to In progress", t.0)
    );
    let r = tasks::toggle_done(&app.state, M, t).unwrap();
    assert_eq!(r.undo.unwrap().label, format!("Mark done #{}", t.0));
    let r = tasks::toggle_done(&app.state, M, t).unwrap();
    assert_eq!(
        r.patch.as_ref().unwrap().upserts[0].status,
        Status::Doing,
        "reopens to what it was"
    );
    assert_eq!(
        err_code(tasks::set_status(&app.state, M, t, Status::Blocked, None)),
        (ErrorCode::NotAllowed, "Say why it's blocked.".into())
    );
    let r = tasks::set_status(&app.state, M, t, Status::Doing, None).unwrap();
    assert!(r.undo.is_none(), "no change, nothing to undo");
    assert_told(&app);
}

#[test]
fn accept_proposals_moves_each_to_todo() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let ids: Vec<TaskId> = (0..2)
        .map(|i| {
            app.core(|s| {
                s.add_task(
                    airboss_core::NewTask::new(format!("prop {i}")).under(p),
                    &Actor::Agent("t3/x".into()),
                )
                .unwrap()
                .id
            })
        })
        .collect();
    app.sync_vm();
    let patch = tasks::accept_proposals(&app.state, M, ids.clone()).unwrap();
    assert!(
        patch
            .upserts
            .iter()
            .filter(|r| ids.contains(&r.id))
            .all(|r| r.status == Status::Todo)
    );
}

#[test]
fn move_and_reorder_move_within_the_tree() {
    let app = TestApp::new();
    let p = app.add("P", None);
    let a = app.add("A", None);
    let b = app.add("B", None);
    app.sync_vm();
    let r = tasks::move_task(&app.state, M, a, Some(p), None).unwrap();
    assert_eq!(app.state.row(a).unwrap().parent, Some(p));
    assert!(r.undo.is_some());
    let r = tasks::reorder(&app.state, M, b, Direction::Up).unwrap();
    assert_eq!(app.state.row(b).unwrap().position, 1);
    assert!(r.undo.is_some());
    let r = tasks::reorder(&app.state, M, b, Direction::Up).unwrap();
    assert!(
        r.undo.is_none() && r.patch.is_none(),
        "at the top nothing happens"
    );
    let r = tasks::reorder(&app.state, M, a, Direction::Down).unwrap();
    assert!(r.undo.is_none(), "an only child can't move");
    let done = app.add("Done", None);
    app.core(|s| s.set_status(done, Status::Done, None, &Actor::Me).unwrap());
    app.sync_vm();
    assert_eq!(
        err_code(tasks::move_task(&app.state, M, b, Some(done), None)).1,
        "Tasks can't go under a proposed, done or dropped task."
    );
    assert_eq!(
        err_code(tasks::move_task(&app.state, M, p, Some(a), None)),
        (
            ErrorCode::Loop,
            "A task can't go under itself or one of its subtasks.".into()
        )
    );
}

#[test]
fn repos_link_and_unlink() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    let t = app.add("T", None);
    app.sync_vm();
    let p = tasks::link_repo(&app.state, M, t, web).unwrap();
    assert_eq!(p.upserts[0].repos, vec![web]);
    let p = tasks::unlink_repo(&app.state, M, t, web).unwrap();
    assert!(p.upserts[0].repos.is_empty());
    assert_eq!(
        err_code(tasks::unlink_repo(&app.state, M, t, web)).1,
        format!("That repo isn't linked to #{}.", t.0)
    );
}

#[test]
fn dependencies_and_their_errors() {
    let app = TestApp::new();
    let a = app.add("A", None);
    let b = app.add("B", None);
    let child = app.add("Child", Some(a));
    app.sync_vm();
    let p = tasks::add_dependency(&app.state, M, a, b).unwrap();
    assert_eq!(
        p.upserts.iter().find(|r| r.id == a).unwrap().waits_on,
        vec![b]
    );
    assert_eq!(
        err_code(tasks::add_dependency(&app.state, M, b, a)),
        (
            ErrorCode::Loop,
            format!(
                "That would make a loop: #{} already waits on #{}.",
                a.0, b.0
            )
        )
    );
    assert_eq!(
        err_code(tasks::add_dependency(&app.state, M, child, a)),
        (
            ErrorCode::InvalidTarget,
            "A task can't wait on its own parent or subtask.".into()
        )
    );
    tasks::remove_dependency(&app.state, M, a, b).unwrap();
    assert!(app.state.row(a).unwrap().waits_on.is_empty());
}

// ---------------------------------------------------------------------------- repos and links

#[test]
fn add_repos_names_each_from_its_folder_and_reads_its_remote() {
    let app = TestApp::new();
    let dir = temp_dir("repos");
    let web = dir.join("booking-web");
    std::fs::create_dir_all(web.join(".git")).unwrap();
    std::fs::write(
        web.join(".git/config"),
        "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:acme/booking-web.git\n",
    )
    .unwrap();
    let out = repos::add_repos(&app.state, M, vec![web.to_string_lossy().into()]).unwrap();
    assert_eq!(out.repos.len(), 1);
    assert_eq!(out.repos[0].name, "booking-web");
    assert_eq!(
        out.repos[0].remote_url.as_deref(),
        Some("git@github.com:acme/booking-web.git")
    );
    assert_eq!(
        out.patch.repos.as_ref().unwrap().len(),
        1,
        "repos changed with no event"
    );
    let again = repos::add_repos(&app.state, M, vec![web.to_string_lossy().into()]);
    assert_eq!(
        err_code(again).1,
        "A repo with that name or folder is already added."
    );
}

#[test]
fn pick_folder_asks_the_shell() {
    let app = TestApp::new();
    assert_eq!(repos::pick_folder(&app.state, M).unwrap(), None);
    app.shell.answer_folder(Some(PathBuf::from("/src/x")));
    assert_eq!(
        repos::pick_folder(&app.state, M).unwrap(),
        Some("/src/x".into())
    );
}

#[test]
fn add_link_classifies_and_a_pr_lookup_corrects_its_state_in_the_background() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    app.sync.answer_lookup(
        PrRef::parse("acme/web#5").unwrap(),
        Ok(pr_view("acme/web#5", PrState::Merged, None, false)),
    );
    let first = repos::add_link(
        &app.state,
        M,
        t,
        "https://github.com/acme/web/pull/5".into(),
        None,
    )
    .unwrap();
    // Wait for the background lookup's own patch, without touching the store meanwhile (the
    // lock checker can't tell whose lock it is).
    app.shell
        .wait_for(
            Duration::from_secs(2),
            |e| matches!(e, UiEvent::TreePatch(p) if p.seq > first.seq),
        )
        .expect("the lookup never corrected the state");
    // Let the background catch-up finish its rebuild before this thread takes the lock.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        app.core(|s| s.links(t).unwrap())[0].pr_state,
        Some(PrState::Merged)
    );
    repos::add_link(&app.state, M, t, "~/plans/booking.md".into(), None).unwrap();
    let links = app.core(|s| s.links(t).unwrap());
    assert_eq!(links[1].kind, LinkKind::Doc);
    assert_eq!(links[1].title.as_deref(), Some("booking.md"));
    assert_eq!(
        err_code(repos::add_link(&app.state, M, t, "hello".into(), None)),
        (ErrorCode::NotALink, "Paste a link or a file path.".into())
    );
    let id = links[1].id;
    repos::remove_link(&app.state, M, id).unwrap();
    assert_eq!(
        err_code(repos::remove_link(&app.state, M, id)),
        (ErrorCode::NotFound, "That link no longer exists.".into())
    );
    app.assert_lock_free();
}

// ---------------------------------------------------------------------------- Do it

fn app_prepared() -> Prepared {
    Prepared {
        repos: vec![],
        from_subtasks: false,
        needs_choice: false,
        choices: vec![airboss_core::Repo {
            id: RepoId(1),
            name: "web".into(),
            path: "/src/web".into(),
            remote_url: None,
        }],
        branch: Some("task-1-x".into()),
        research_dir: PathBuf::from("/tmp/research"),
        waiting_on: vec![],
        started: vec![],
    }
}

fn started(key: &str, repo: Option<RepoId>, mode: Mode) -> StartedThread {
    StartedThread {
        key: ThreadKey {
            source: Source::T3,
            r#ref: key.into(),
        },
        mode,
        repo,
        title: key.into(),
        linked: true,
    }
}

#[test]
fn do_it_holds_an_in_flight_guard_until_start_returns() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    app.gate.close();
    let state = app.state.clone();
    let first = std::thread::spawn(move || launch::do_it(&state, M, t, None, None));
    while app.gate.entered.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        err_code(launch::do_it(&app.state, M, t, None, None)),
        (
            ErrorCode::AlreadyStarting,
            "Already starting — check T3 in a moment.".into()
        )
    );
    assert_eq!(
        err_code(launch::plan_it(&app.state, M, t, None)).0,
        ErrorCode::AlreadyStarting
    );
    app.gate.open();
    assert!(matches!(
        first.join().unwrap().unwrap(),
        DoItResult::Started { .. }
    ));
    // The guard is gone once start returned.
    assert!(launch::do_it(&app.state, M, t, None, None).is_ok());
    app.assert_lock_free();
}

#[test]
fn do_it_asks_where_then_starts_in_the_chosen_repo_or_research_only() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    let t = app.add("T", None);
    app.sync_vm();
    app.launch.set_prepare(Ok(Prepared {
        needs_choice: true,
        ..app_prepared()
    }));
    match launch::do_it(&app.state, M, t, None, None).unwrap() {
        DoItResult::NeedsChoice {
            choices,
            research_dir,
        } => {
            assert_eq!(choices[0].name, "web");
            assert_eq!(research_dir, "/tmp/research");
        }
        other => panic!("{other:?}"),
    }
    assert!(app.launch.starts().is_empty());
    app.launch.set_start(Ok(LaunchReport {
        started: vec![started("th1", Some(web), Mode::DoIt)],
        skipped: vec![],
        failed: vec![],
        branch: Some("task-1-t".into()),
        moved_to_doing: true,
    }));
    match launch::do_it(&app.state, M, t, Some(DoItPlace::Repo(web)), None).unwrap() {
        DoItResult::Started { report, .. } => assert_eq!(report.started.len(), 1),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        app.core(|s| s.task(t).unwrap().repos),
        vec![web],
        "linked as me"
    );
    assert_eq!(app.launch.starts()[0].place, Place::TaskRepos);
    launch::do_it(&app.state, M, t, Some(DoItPlace::NoRepo), Some(true)).unwrap();
    let last = app.launch.starts().last().unwrap().clone();
    assert_eq!(last.place, Place::NoRepo(PathBuf::from("/tmp/research")));
    assert!(last.again);
}

#[test]
fn a_partial_report_offers_add_to_t3_and_from_quick_add_shows_the_main_window() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    let api = repo(&app, "api");
    let t = app.add("T", None);
    app.sync_vm();
    app.launch.set_start(Ok(LaunchReport {
        started: vec![started("th1", Some(web), Mode::DoIt)],
        skipped: vec![],
        failed: vec![(
            Some(api),
            ServiceError::new(SvcCode::NotAProject, "api isn't a T3 project yet."),
        )],
        branch: Some("task-1-t".into()),
        moved_to_doing: true,
    }));
    let DoItResult::Started { report, .. } =
        launch::do_it(&app.state, Caller::QuickAdd, t, None, None).unwrap()
    else {
        panic!()
    };
    assert_eq!(report.failed[0].1.code, SvcCode::NotAProject);
    // From quick add, the failure shows in the main window, as a toast with the fixes.
    let toast = app
        .shell
        .events()
        .into_iter()
        .find_map(|e| match e {
            UiEvent::Toast(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        toast.text,
        "Started in web. api failed: api isn't a T3 project yet."
    );
    assert_eq!(toast.tone, Some(ToastTone::Warning));
    assert_eq!(toast.actions[0].command, "add_to_t3");
    assert_eq!(toast.actions[0].args, ToastArgs::Repo { repo: api });
    assert_eq!(toast.actions[1].command, "do_it");
    // The UI calls `api[command](...Object.values(args))`, so the order matters.
    assert_eq!(
        serde_json::to_string(&toast.actions[1].args).unwrap(),
        format!(r#"{{"id":{},"place":null,"again":false}}"#, t.0)
    );
    assert!(app.shell.calls().contains(&"show_main".to_string()));
    assert!(
        app.shell
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::NavOpen(n) if n.task == Some(t)))
    );
    // Add to T3, then retry.
    launch::add_to_t3(&app.state, M, api).unwrap();
    assert!(app.launch.calls().contains(&format!("add_to_t3({api:?})")));
}

#[test]
fn the_toast_table_for_every_report() {
    let name = |r: Option<RepoId>| match r {
        Some(RepoId(1)) => "booking-web".to_string(),
        Some(RepoId(2)) => "booking-api".to_string(),
        _ => "research".to_string(),
    };
    let t = TaskId(142);
    let rep = |started, skipped, failed| LaunchReport {
        started,
        skipped,
        failed,
        branch: Some("task-142-booking-flow".into()),
        moved_to_doing: false,
    };
    let text = |m, r: &LaunchReport| launch::report_toast(t, m, r, &name).text;
    assert_eq!(
        text(
            Mode::DoIt,
            &rep(
                vec![
                    started("a", Some(RepoId(2)), Mode::DoIt),
                    started("b", Some(RepoId(1)), Mode::DoIt)
                ],
                vec![],
                vec![]
            )
        ),
        "Started 2 threads for #142 on task-142-booking-flow"
    );
    assert_eq!(
        text(
            Mode::DoIt,
            &rep(vec![started("a", None, Mode::DoIt)], vec![], vec![])
        ),
        "Started a research thread for #142"
    );
    assert_eq!(
        text(
            Mode::PlanIt,
            &rep(vec![started("a", None, Mode::PlanIt)], vec![], vec![])
        ),
        "Started a planning thread for #142. Its steps become subtasks when you settle the plan."
    );
    let skipped = launch::report_toast(
        t,
        Mode::DoIt,
        &rep(vec![], vec![RepoId(1), RepoId(2)], vec![]),
        &name,
    );
    assert_eq!(
        skipped.text,
        "#142 already has threads in booking-web and booking-api"
    );
    assert_eq!(skipped.actions[0].command, "do_it");
    assert_eq!(
        skipped.actions[0].args,
        ToastArgs::DoIt {
            id: t,
            place: None,
            again: true
        }
    );
    let failed = launch::report_toast(
        t,
        Mode::DoIt,
        &rep(
            vec![],
            vec![],
            vec![(
                Some(RepoId(1)),
                ServiceError::new(SvcCode::T3Failed, "t3 said no"),
            )],
        ),
        &name,
    );
    assert_eq!(failed.text, "Couldn't start #142: t3 said no");
    assert_eq!(failed.actions[0].label, "Retry");
}

#[test]
fn a_closed_task_offers_reopen_and_start() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.core(|s| s.set_status(t, Status::Done, None, &Actor::Me).unwrap());
    app.sync_vm();
    app.launch
        .set_prepare(Err(ServiceError::new(SvcCode::TaskClosed, "142 is done.")));
    assert_eq!(
        err_code(launch::do_it(&app.state, M, t, None, None)).0,
        ErrorCode::TaskClosed
    );
    app.launch.set_prepare(Ok(app_prepared()));
    launch::reopen_and_start(&app.state, M, t).unwrap();
    assert_eq!(app.core(|s| s.task(t).unwrap().status), Status::Todo);
    assert_eq!(app.launch.starts().len(), 1);
}

#[test]
fn a_service_error_passes_through_with_its_own_code_and_message() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    app.launch.set_start(Err(ServiceError::new(
        SvcCode::T3NotRunning,
        "Open T3 Code, then try again.",
    )));
    assert_eq!(
        err_code(launch::plan_it(&app.state, M, t, None)),
        (
            ErrorCode::T3NotRunning,
            "Open T3 Code, then try again.".into()
        )
    );
}

// ---------------------------------------------------------------------------- needs me

#[test]
fn dismiss_snooze_and_restore_go_through_attention() {
    let app = TestApp::new();
    let t = app.add("T", None);
    app.attention.set_items(vec![
        needs_item("q", NeedsKind::Question, t, common::NOW),
        needs_item("quiet", NeedsKind::Quiet, t, common::NOW),
    ]);
    app.state.catch_up_services();
    let w = needs::needs_dismiss(&app.state, M, "q".into()).unwrap();
    assert!(w.patch.is_none());
    assert_eq!(w.undo.unwrap().label, format!("Dismiss #{}", t.0));
    needs::needs_snooze(&app.state, M, "quiet".into(), None).unwrap();
    assert!(
        app.attention
            .calls()
            .contains(&"snooze(quiet, None)".to_string())
    );
    needs::needs_restore(&app.state, M, "q".into()).unwrap();
    assert_eq!(app.attention.needs_me().count, 1);
    assert_eq!(
        err_code(needs::needs_restore(&app.state, M, "never".into())).0,
        ErrorCode::NotFound
    );
}

// ---------------------------------------------------------------------------- app commands

#[test]
fn open_thread_and_open_url_go_through_the_allowlist() {
    let app = TestApp::new();
    appcmd::open_thread(
        &app.state,
        M,
        ThreadKey {
            source: Source::T3,
            r#ref: "abc".into(),
        },
    )
    .unwrap();
    assert!(
        app.shell
            .calls()
            .contains(&"open_url(t3code://threads/fake-environment/abc with /Applications/T3 Code (Beta).app)".to_string())
    );
    assert_eq!(
        err_code(appcmd::open_thread(
            &app.state,
            M,
            ThreadKey {
                source: Source::Claude,
                r#ref: "s".into()
            }
        ))
        .0,
        ErrorCode::NotFound
    );
    appcmd::open_url(
        &app.state,
        M,
        OpenTarget::Url("https://github.com/acme/web/pull/1".into()),
    )
    .unwrap();
    assert_eq!(
        err_code(appcmd::open_url(
            &app.state,
            M,
            OpenTarget::Url("javascript:alert(1)".into()),
        ))
        .0,
        ErrorCode::UrlNotAllowed
    );
    let t = app.add("T", None);
    let link = app.core(|s| {
        s.link(t, NewLink::new(LinkKind::Doc, "~/plan.md"), &Actor::Me)
            .unwrap()
    });
    // A `~` path opens from the home folder, once it's there.
    assert_eq!(
        err_code(appcmd::open_url(&app.state, M, OpenTarget::Link(link.id))),
        (ErrorCode::NotFound, "That file no longer exists.".into())
    );
    std::fs::write(app.home.join("plan.md"), "# plan").unwrap();
    appcmd::open_url(&app.state, M, OpenTarget::Link(link.id)).unwrap();
    let plan = app.home.join("plan.md").canonicalize().unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        &format!(
            "open_file({} in /System/Applications/TextEdit.app)",
            plan.display()
        )
    );
}

#[test]
fn recheck_copy_and_install_cli() {
    let app = TestApp::new();
    appcmd::recheck_connections(&app.state, M).unwrap();
    for calls in [app.sync.calls(), app.launch.calls(), app.attention.calls()] {
        assert!(calls.contains(&"recheck".to_string()));
    }
    appcmd::copy_text(&app.state, M, CopyWhat::TaskNumber(TaskId(144))).unwrap();
    appcmd::copy_text(&app.state, M, CopyWhat::SetupCommand).unwrap();
    appcmd::copy_text(&app.state, M, CopyWhat::AgentNote).unwrap();
    let calls = app.shell.calls();
    assert!(calls.contains(&"copy_text(144)".to_string()));
    assert!(calls.contains(
        &"copy_text(claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp)".to_string()
    ));
    assert!(calls.iter().any(|c| c.starts_with("copy_text(## Airboss")));

    assert_eq!(
        err_code(appcmd::install_cli(&app.state, M)).0,
        ErrorCode::NotFound
    );
    let sidecar = temp_dir("bundle").join("airboss");
    std::fs::write(&sidecar, b"#!/bin/sh\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    app.shell.set_sidecar(Some(sidecar.clone()));
    let out = appcmd::install_cli(&app.state, M).unwrap();
    let link = app.home.join(".local/bin/airboss");
    assert_eq!(out.path, link.to_string_lossy());
    assert_eq!(std::fs::read_link(&link).unwrap(), sidecar);
    // Installing again replaces our own link. No agent tool has the server yet (#9).
    appcmd::install_cli(&app.state, M).unwrap();
    assert!(
        app.state
            .connection_rows()
            .iter()
            .any(|c| c.name == "Agents" && !c.ok && c.state == "Not connected")
    );
    std::fs::remove_file(&link).unwrap();
    std::fs::write(&link, b"someone else's").unwrap();
    assert_eq!(
        err_code(appcmd::install_cli(&app.state, M)).0,
        ErrorCode::AlreadyExists
    );
}

fn agents(app: &TestApp) -> Connection {
    app.state.refresh_connections();
    app.state
        .connection_rows()
        .into_iter()
        .find(|c| c.name == "Agents")
        .unwrap()
}

/// Audit should-fix 9: a dangling link (or one to a file that can't run) isn't Ready, and
/// Ready says when an agent last reported, or that none has.
#[test]
fn agents_are_ready_only_with_a_working_cli_and_say_when_one_last_reported() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let app = TestApp::new();
    let bin = app.home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let link = bin.join("airboss");
    symlink(app.home.join("Old.app/Contents/MacOS/airboss"), &link).unwrap();
    let a = agents(&app);
    assert_eq!((a.ok, a.state.as_str()), (false, "Link broken"), "{a:?}");
    assert!(!detail::bootstrap(&app.state, M).unwrap().cli.installed);

    let cli = temp_dir("cli").join("airboss");
    std::fs::write(&cli, b"").unwrap();
    std::fs::remove_file(&link).unwrap();
    symlink(&cli, &link).unwrap();
    assert!(
        !agents(&app).ok,
        "an empty, non-executable file isn't a CLI"
    );

    std::fs::write(&cli, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        app.home.join(".claude.json"),
        r#"{"mcpServers":{"airboss":{}}}"#,
    )
    .unwrap();
    let a = agents(&app);
    assert_eq!((a.ok, a.state.as_str()), (true, "Ready"), "{a:?}");
    assert_eq!(a.detail, "No agent has reported yet.");
    let b = detail::bootstrap(&app.state, M).unwrap();
    assert!(b.cli.installed);
    assert_eq!(b.agent_report, None);

    // An agent reports: the time arrives with connections:changed.
    let t = app.add("T", None);
    app.core(|s| {
        s.add_note(t, "started", &Actor::Agent("t3/thread-1".into()))
            .unwrap()
    });
    app.sync_vm();
    let at = app.core(|s| s.history(t).unwrap().last().unwrap().at.clone());
    assert_eq!(
        detail::bootstrap(&app.state, M).unwrap().agent_report,
        Some(at.clone())
    );
    assert!(app.shell.events().iter().any(|e| matches!(
        e,
        UiEvent::ConnectionsChanged(c) if c.agent_report.as_deref() == Some(at.as_str())
    )));
    // My own later writes don't count as a report.
    tasks::rename_task(&app.state, M, t, "Renamed".into()).unwrap();
    assert_eq!(
        detail::bootstrap(&app.state, M).unwrap().agent_report,
        Some(at)
    );
}

/// A working `~/.local/bin/airboss` under the test's home.
fn install_fake_cli(app: &TestApp) {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let cli = temp_dir("cli").join("airboss");
    std::fs::write(&cli, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bin = app.home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    symlink(&cli, bin.join("airboss")).unwrap();
}

/// A fake `claude` that writes its argv, one per line, and `$HOME` to `<dir>/args`, then runs
/// `body`. It never touches the real `~/.claude.json`: `HOME` is the test's.
fn fake_claude(body: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("claude");
    let args = dir.join("args");
    let script = dir.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\necho \"HOME=$HOME\" >> '{}'\n{body}\n",
            args.display(),
            args.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, args)
}

/// #9: Ready needs an agent tool that has the server, not only the CLI.
#[test]
fn agents_are_not_connected_until_claude_or_codex_has_the_server() {
    let app = TestApp::new();
    install_fake_cli(&app);
    let a = agents(&app);
    assert_eq!((a.ok, a.state.as_str()), (false, "Not connected"), "{a:?}");
    assert_eq!(a.detail, "Agents can't report to Airboss yet.");
    let b = detail::bootstrap(&app.state, M).unwrap();
    assert_eq!((b.agents.claude, b.agents.codex), (false, false));

    // A malformed file is not connected, and nothing breaks.
    std::fs::write(app.home.join(".claude.json"), "{\"mcpServers\": ").unwrap();
    assert!(!agents(&app).ok);

    // Codex alone is enough.
    std::fs::create_dir_all(app.home.join(".codex")).unwrap();
    std::fs::write(
        app.home.join(".codex/config.toml"),
        "[mcp_servers.airboss]\ncommand = \"airboss\"\nargs = [\"mcp\"]\n",
    )
    .unwrap();
    let a = agents(&app);
    assert_eq!((a.ok, a.state.as_str()), (true, "Ready"), "{a:?}");
    assert_eq!(a.detail, "No agent has reported yet.");
    let b = detail::bootstrap(&app.state, M).unwrap();
    assert_eq!((b.agents.claude, b.agents.codex), (false, true));
    assert!(app.shell.events().iter().any(|e| matches!(
        e,
        UiEvent::ConnectionsChanged(c) if c.agents.codex && !c.agents.claude
    )));
}

#[test]
fn connect_claude_runs_claude_mcp_add_with_the_absolute_cli_path() {
    // The fake registers the server the way the real one does: in $HOME/.claude.json.
    let (claude, args) = fake_claude(
        "printf '{\"mcpServers\":{\"airboss\":{\"command\":\"x\"}}}' > \"$HOME/.claude.json\"\necho 'Added stdio MCP server airboss'",
    );
    let app = TestApp::new().with_claude(claude);
    // Without the CLI there's nothing to point Claude Code at.
    let (code, message) = err_code(appcmd::connect_claude(&app.state, M));
    assert_eq!(code, ErrorCode::NotFound);
    assert!(
        message.contains("Install the command-line tool"),
        "{message}"
    );
    assert!(!args.exists(), "claude ran without the CLI");

    install_fake_cli(&app);
    assert!(!agents(&app).ok);
    appcmd::connect_claude(&app.state, M).unwrap();
    let bin = app.home.join(".local/bin/airboss");
    assert_eq!(
        std::fs::read_to_string(&args).unwrap(),
        format!(
            "mcp\nadd\n--scope\nuser\nairboss\n--\n{}\nmcp\nHOME={}\n",
            bin.display(),
            app.home.display()
        )
    );
    let a = agents(&app);
    assert_eq!((a.ok, a.state.as_str()), (true, "Ready"), "{a:?}");
    assert!(app.shell.events().iter().any(|e| matches!(
        e,
        UiEvent::ConnectionsChanged(c) if c.agents.claude
    )));

    // Already registered: success, and claude isn't run again.
    std::fs::remove_file(&args).unwrap();
    appcmd::connect_claude(&app.state, M).unwrap();
    assert!(!args.exists());
}

#[test]
fn connect_claude_reports_a_failure_by_its_first_line_and_takes_already_exists_as_done() {
    let (claude, args) = fake_claude(
        "echo '' >&2\necho 'Error: Invalid scope: user' >&2\necho 'Usage: claude mcp add' >&2\nexit 1",
    );
    let app = TestApp::new().with_claude(claude);
    install_fake_cli(&app);
    let (code, message) = err_code(appcmd::connect_claude(&app.state, M));
    assert_eq!(code, ErrorCode::Internal);
    assert_eq!(
        message,
        "Couldn't connect Claude Code: Error: Invalid scope: user"
    );
    assert!(args.exists());
    assert!(!agents(&app).ok);

    // Registered in a way Airboss doesn't read (another config dir): claude says so; that's done.
    let (claude, _) =
        fake_claude("echo 'MCP server airboss already exists in user config' >&2\nexit 1");
    let app = app.with_claude(claude);
    appcmd::connect_claude(&app.state, M).unwrap();

    // No claude at all.
    let app = app.with_claude(PathBuf::from("/nonexistent/airboss-test/claude"));
    let (code, message) = err_code(appcmd::connect_claude(&app.state, M));
    assert_eq!(code, ErrorCode::NotFound);
    assert!(message.contains("Copy the setup command"), "{message}");
}

#[test]
fn copy_codex_setup_names_the_cli_by_its_absolute_path_and_passes_the_thread_id() {
    let app = TestApp::new();
    appcmd::copy_text(&app.state, M, CopyWhat::CodexSetup).unwrap();
    let bin = app.home.join(".local/bin/airboss");
    let expected = format!(
        "copy_text([mcp_servers.airboss]\ncommand = \"{}\"\nargs = [\"mcp\"]\nenv_vars = [\"T3CODE_THREAD_ID\", \"HC_TERMINAL_ID\"]\n)",
        bin.display()
    );
    assert!(
        app.shell.calls().contains(&expected),
        "{:?}",
        app.shell.calls()
    );
}

/// Settings' "~/.local/bin isn't on your PATH yet" follows the user's interactive login
/// shell (so `~/.zshrc` counts), checked in the background at start and after an install, and
/// sent with the connections.
#[test]
fn the_path_check_reads_zshrc_and_reaches_the_ui() {
    use std::os::unix::fs::PermissionsExt;
    let mut app = TestApp::new();
    let zdot = temp_dir("zdot");
    let bin = app.home.join(".local/bin");
    std::fs::write(
        zdot.join(".zshrc"),
        format!("export PATH=\"$PATH:{}\"\n", bin.display()),
    )
    .unwrap();
    // A shell that reads the throwaway ZDOTDIR, never the user's own startup files.
    let shell = temp_dir("shell").join("zsh");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\nZDOTDIR='{}' exec /bin/zsh \"$@\"\n",
            zdot.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
    app = app.with_shell(shell);
    assert!(!detail::bootstrap(&app.state, M).unwrap().cli.on_path);
    app.state.start_background();
    let sent = app.shell.wait_for(
        Duration::from_secs(10),
        |e| matches!(e, UiEvent::ConnectionsChanged(c) if c.cli.on_path),
    );
    assert!(sent.is_some(), "{:?}", app.shell.events());
    assert!(detail::bootstrap(&app.state, M).unwrap().cli.on_path);
}

/// attention's "Needs me · Didn't start" row (its table didn't open) reaches the footer.
#[test]
fn a_needs_me_that_didnt_start_shows_in_the_footer() {
    let app = TestApp::new();
    app.attention.set_connections(vec![Connection {
        name: "Needs me".into(),
        ok: false,
        state: "Didn't start".into(),
        detail: "Its table couldn't open: disk I/O error".into(),
        fix: Some("Quit and reopen Airboss. If it happens again, see app.log.".into()),
    }]);
    app.state.catch_up_services();
    let rows = app.state.connection_rows();
    let row = rows.iter().find(|r| r.name == "Needs me").unwrap();
    assert!(!row.ok && row.state == "Didn't start", "{row:?}");
    assert!(app.shell.events().iter().any(|e| matches!(
        e,
        UiEvent::ConnectionsChanged(c) if c.connections.iter().any(|r| r.name == "Needs me")
    )));
}

/// The newest agent report is found at start, behind later events of mine.
#[test]
fn the_last_agent_report_is_found_at_start() {
    let mut store = airboss_core::Store::open_in_memory().unwrap();
    let t = store
        .add_task(airboss_core::NewTask::new("T"), &Actor::Me)
        .unwrap()
        .id;
    store
        .add_note(t, "working", &Actor::Agent("t3/x".into()))
        .unwrap();
    let at = store.history(t).unwrap().last().unwrap().at.clone();
    for i in 0..300 {
        store
            .edit_task(
                t,
                airboss_core::TaskEdit::default().title(format!("T{i}")),
                &Actor::Me,
            )
            .unwrap();
    }
    let app = TestApp::with_store(store);
    assert_eq!(
        detail::bootstrap(&app.state, M).unwrap().agent_report,
        Some(at)
    );
}

#[test]
fn set_settings_refuses_reserved_and_taken_hotkeys_and_applies_the_rest() {
    let app = TestApp::new();
    let mut s = appcmd::get_settings(&app.state, M).unwrap();
    s.hotkey = "Control+Space".into();
    assert_eq!(
        err_code(appcmd::set_settings(&app.state, M, s.clone())).0,
        ErrorCode::HotkeyReserved
    );
    s.hotkey = "Control+Alt+K".into();
    app.shell.refuse_hotkeys(true);
    assert_eq!(
        err_code(appcmd::set_settings(&app.state, M, s.clone())),
        (
            ErrorCode::HotkeyInUse,
            "That key is in use by another app. Pick another.".into()
        )
    );
    assert_eq!(app.state.settings().hotkey, "Control+Alt+N", "unchanged");
    app.shell.refuse_hotkeys(false);
    s.open_at_login = false;
    s.appearance = Appearance::Light;
    let saved = appcmd::set_settings(&app.state, M, s).unwrap();
    assert_eq!(saved.hotkey, "Control+Alt+K");
    let calls = app.shell.calls();
    assert!(calls.contains(&"register_hotkey(Control+Alt+N -> Control+Alt+K)".to_string()));
    assert!(calls.contains(&"set_autostart(false)".to_string()));
    assert!(calls.contains(&"apply_appearance(Light)".to_string()));
}

#[test]
fn a_hotkey_taken_at_start_shows_in_the_footer_until_settings_picks_one_that_works() {
    let app = TestApp::new();
    app.shell.refuse_hotkeys(true);
    app.state.register_hotkey_at_start();
    let row = app
        .state
        .connection_rows()
        .into_iter()
        .find(|r| r.name == "Quick add")
        .expect("a Quick add row");
    assert!(!row.ok);
    assert_eq!(row.state, "Shortcut taken");
    assert_eq!(
        row.fix.as_deref(),
        Some("Quick add shortcut ⌃⌥N is taken by another app. Change it in Settings.")
    );
    assert!(app.shell.events().iter().any(|e| matches!(
        e,
        UiEvent::ConnectionsChanged(c) if c.connections.iter().any(|r| r.name == "Quick add")
    )));
    let quick_add_row = |app: &TestApp| {
        app.state
            .connection_rows()
            .iter()
            .any(|r| r.name == "Quick add")
    };
    // Another key that's taken changes nothing.
    let mut s = appcmd::get_settings(&app.state, M).unwrap();
    s.hotkey = "Control+Alt+K".into();
    assert!(appcmd::set_settings(&app.state, M, s.clone()).is_err());
    assert!(quick_add_row(&app));
    // One that works clears it.
    app.shell.refuse_hotkeys(false);
    appcmd::set_settings(&app.state, M, s).unwrap();
    assert!(!quick_add_row(&app));
}

#[test]
fn a_free_hotkey_at_start_adds_no_row() {
    let app = TestApp::new();
    app.state.register_hotkey_at_start();
    assert!(
        app.shell
            .calls()
            .contains(&"register_hotkey( -> Control+Alt+N)".to_string())
    );
    assert!(
        !app.state
            .connection_rows()
            .iter()
            .any(|r| r.name == "Quick add")
    );
}

#[test]
fn folds_focus_and_windows() {
    let app = TestApp::new();
    appcmd::set_folded(&app.state, M, vec![TaskId(1), TaskId(2)], true).unwrap();
    appcmd::set_folded(&app.state, M, vec![TaskId(1)], false).unwrap();
    assert_eq!(app.state.settings().folded, vec![TaskId(2)]);

    appcmd::set_focus_context(&app.state, M, focus(Zone::Field)).unwrap();
    assert!(
        app.shell
            .calls()
            .last()
            .unwrap()
            .contains("task_items: false")
    );
    appcmd::set_focus_context(&app.state, M, focus(Zone::List)).unwrap();
    assert!(
        app.shell
            .calls()
            .last()
            .unwrap()
            .contains("task_items: true, selection: true")
    );

    appcmd::ui_ready(&app.state, M, WindowName::Main).unwrap();
    appcmd::ui_ready(&app.state, M, WindowName::Main).unwrap();
    assert_eq!(
        app.shell
            .calls()
            .iter()
            .filter(|c| *c == "show_main")
            .count(),
        1
    );

    appcmd::quickadd_shown(&app.state, Caller::QuickAdd).unwrap();
    appcmd::quickadd_resize(&app.state, Caller::QuickAdd, 240.0).unwrap();
    appcmd::quickadd_hide(&app.state, Caller::QuickAdd).unwrap();
    assert!(
        app.shell
            .calls()
            .contains(&"resize_quickadd(240)".to_string())
    );
    assert!(app.shell.calls().contains(&"hide_quickadd".to_string()));
    assert!(
        app.shell
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::QuickAddReset(r) if r.keep_text == Some(false) && r.hotkey == "⌃⌥N"))
    );

    appcmd::show_main(&app.state, M, Some("needs".into()), None, Some("q".into())).unwrap();
    assert!(
        app.shell
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::NavOpen(n) if n.needs_key.as_deref() == Some("q")))
    );
}

#[test]
fn connections_merge_rows_by_name_and_follow_availability() {
    let app = TestApp::new();
    app.sync.set_connections(vec![
        Connection {
            name: "GitHub".into(),
            ok: true,
            state: "Signed in".into(),
            detail: "PRs".into(),
            fix: None,
        },
        Connection {
            name: "T3 Code".into(),
            ok: true,
            state: "Found".into(),
            detail: "threads".into(),
            fix: None,
        },
    ]);
    app.launch.set_availability(Availability::Missing {
        reason: "Not installed".into(),
    });
    app.state.catch_up_services();
    let rows = app.state.connection_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["T3 Code", "GitHub", "Agents"]);
    assert!(!rows[0].ok && rows[0].state == "Not found", "{:?}", rows[0]);
    assert!(
        app.shell
            .events()
            .iter()
            .any(|e| matches!(e, UiEvent::ConnectionsChanged(_)))
    );
}

// ---------------------------------------------------------------------------- errors

#[test]
fn a_panic_in_a_command_is_an_internal_error() {
    let r: Result<(), AppError> = airboss_app::ipc::guarded_call(|| panic!("boom"));
    assert_eq!(
        err_code(r),
        (
            ErrorCode::Internal,
            "Something went wrong inside Airboss; details are in the log.".into()
        )
    );
}

#[test]
fn commands_run_by_name_with_json_as_invoke_sends_them() {
    let app = TestApp::new();
    // Tauri's camelCase argument keys; snake_case fields inside payloads.
    let out = app
        .call(
            "create_task",
            serde_json::json!({"task": {"title": "From JSON", "body": null, "parent": null, "repos": [], "links": [], "then": "none"}}),
        )
        .unwrap();
    let id = out["id"].as_i64().unwrap();
    assert!(out["link_errors"].as_array().unwrap().is_empty());
    let detail = app
        .call(
            "task_detail",
            serde_json::json!({"id": id, "req": 3, "historyLimit": 30}),
        )
        .unwrap();
    assert_eq!(detail["req"], 3);
    assert_eq!(detail["task"]["title"], "From JSON");
    let r = app
        .call(
            "set_status",
            serde_json::json!({"id": id, "status": "in_review", "note": null}),
        )
        .unwrap();
    assert_eq!(r["patch"]["upserts"][0]["status"], "in_review");
    assert_eq!(r["undo"]["label"], format!("Set #{id} to In review"));
    let other = app.add("Other", None);
    app.call(
        "add_dependency",
        serde_json::json!({"id": id, "waitsOn": other.0}),
    )
    .unwrap();
    app.call(
        "open_url",
        serde_json::json!({"target": {"url": "https://example.com"}}),
    )
    .unwrap();
    app.call(
        "copy_text",
        serde_json::json!({"what": {"task_number": id}}),
    )
    .unwrap();
    let d = app
        .call(
            "do_it",
            serde_json::json!({"id": id, "place": {"repo": 1}, "again": false}),
        )
        .unwrap_err();
    assert_eq!(d.code, ErrorCode::NotFound, "repo 1 doesn't exist");
    app.call(
        "show_main",
        serde_json::json!({"view": "needs", "task": null, "needsKey": "q"}),
    )
    .unwrap();
    assert!(app.call("no_such_command", serde_json::json!({})).is_err());
    let _ = Arc::strong_count(&app.state);
}

#[test]
fn a_write_with_nothing_new_returns_a_patch_the_ui_always_skips() {
    // The worker can catch up first and send the write's patch as `tree:patch`; the write's
    // own answer must not carry that seq, or the UI (which applies a patch only if its seq is
    // greater than the last applied) would drop the real patch if the answer came first.
    let app = TestApp::new();
    let t = app.add("T", None);
    app.sync_vm();
    app.core(|s| {
        s.edit_task(
            t,
            airboss_core::TaskEdit::default().title("Renamed"),
            &Actor::Me,
        )
        .unwrap()
    });
    let event = app.state.catch_up().unwrap().unwrap(); // the worker's tree:patch
    let answer = tasks::rename_task(&app.state, M, t, "Renamed".into()).unwrap();
    assert!(answer.upserts.is_empty());
    assert_eq!(answer.seq, 0);
    // The UI's rule, with the answer arriving first and the event second.
    let mut applied = 0u64;
    let mut apply = |seq: u64| {
        if seq > applied {
            applied = seq;
            true
        } else {
            false
        }
    };
    assert!(!apply(answer.seq));
    assert!(apply(event.seq), "the real patch still applies");
}

#[test]
fn file_links_that_would_run_something_are_shown_in_finder() {
    use std::os::unix::fs::PermissionsExt;
    let app = TestApp::new();
    let dir = temp_dir("open");
    let doc = dir.join("plan.md");
    std::fs::write(&doc, "# plan").unwrap();
    let script = dir.join("run me.command");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    let exe = dir.join("tool");
    std::fs::write(&exe, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bundle = dir.join("Thing.app");
    std::fs::create_dir_all(&bundle).unwrap();
    let url = |p: &Path| airboss_app::classify::file_url(p);
    let real = |p: &Path| p.canonicalize().unwrap();
    appcmd::open_url(&app.state, M, OpenTarget::Url(url(&doc))).unwrap();
    for p in [&script, &exe, &bundle, &dir] {
        appcmd::open_url(&app.state, M, OpenTarget::Url(url(p))).unwrap();
    }
    let calls = app.shell.calls();
    assert!(
        calls.contains(&format!(
            "open_file({} in /System/Applications/TextEdit.app)",
            real(&doc).display()
        )),
        "{calls:?}"
    );
    for p in [&script, &exe, &bundle, &dir] {
        assert!(
            calls.contains(&format!("reveal({})", real(p).display())),
            "{} should be revealed: {calls:?}",
            p.display()
        );
    }
    assert_eq!(
        calls.iter().filter(|c| c.starts_with("open_")).count(),
        1,
        "only the document opens: {calls:?}"
    );
}

/// The second audit's medium finding: an agent's `Evil.app` claiming T3 Code's bundle id and
/// the `t3code` scheme gets registered as soon as Finder reveals its folder. Open thread then
/// checks the handler's signature and refuses an app that isn't T3 Code, offering the link.
#[test]
fn open_thread_refuses_a_handler_that_isnt_t3_code() {
    let app = TestApp::new();
    let check = app.shell.app_check_fake();
    let key = ThreadKey {
        source: Source::T3,
        r#ref: "abc".into(),
    };
    appcmd::open_thread(&app.state, M, key.clone()).unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        "open_url(t3code://threads/fake-environment/abc with /Applications/T3 Code (Beta).app)"
    );
    check.set_handler("t3code", Some("/Users/me/Code/web/Evil.app".into()));
    let e = appcmd::open_thread(&app.state, M, key).unwrap_err();
    assert_eq!(e.code, ErrorCode::AppUnverified);
    assert_eq!(
        e.message,
        "Couldn't verify the app that opens T3 links. Open the thread from T3 Code."
    );
    assert_eq!(
        e.detail.as_deref(),
        Some("t3code://threads/fake-environment/abc")
    );
    assert_eq!(
        app.shell
            .calls()
            .iter()
            .filter(|c| c.starts_with("open_url"))
            .count(),
        1
    );
    // "Copy link" copies it.
    appcmd::copy_text(&app.state, M, CopyWhat::Link(e.detail.unwrap())).unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        "copy_text(t3code://threads/fake-environment/abc)"
    );
}

#[test]
fn a_web_link_opens_only_in_a_verified_browser_outside_the_home_folder() {
    let app = TestApp::new();
    let check = app.shell.app_check_fake();
    let url = "https://github.com/acme/web/pull/1";
    appcmd::open_url(&app.state, M, OpenTarget::Url(url.into())).unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        &format!("open_url({url} with /Applications/Safari.app)")
    );
    let fake = app.home.join("Downloads/Browser.app");
    check.set_handler("https", Some(fake.clone()));
    check.trust(fake, airboss_app::handlers::BROWSER_REQUIREMENT);
    let e = appcmd::open_url(&app.state, M, OpenTarget::Url(url.into())).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (
            ErrorCode::AppUnverified,
            "Couldn't verify your browser. Copy the link instead."
        )
    );
}

/// The final audit's B1: an agent stores a link through MCP `link`, and ⌘O must never run it.
#[test]
fn agent_links_that_would_run_code_never_reach_open() {
    use std::os::unix::fs::PermissionsExt;
    let app = TestApp::new();
    let dir = temp_dir("b1");
    let script = dir.join("x.command");
    std::fs::write(&script, "#!/bin/sh\nopen -a Calculator\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let t = app.add("T", None);
    let base = airboss_app::classify::file_url(&script);
    for url in [
        format!("{base}?a"),
        "file:/System/Applications/Calculator.app?://".to_string(),
        base.replacen("file://", "file://localhost", 1),
        format!("{base}#x"),
        "http://example.com/x.command".to_string(),
    ] {
        let agent = Actor::Agent("t3/thread-1".into());
        let link = app.core(|s| {
            s.link(
                t,
                NewLink::new(LinkKind::Doc, "x").with_url(url.as_str()),
                &agent,
            )
            .unwrap()
        });
        let (code, message) = err_code(appcmd::open_url(&app.state, M, OpenTarget::Link(link.id)));
        assert_eq!(code, ErrorCode::UrlNotAllowed, "{url}");
        assert_eq!(
            message, "Airboss only opens https links, files and T3 threads.",
            "{url}"
        );
    }
    assert!(
        app.shell.calls().iter().all(|c| !c.starts_with("open_")),
        "nothing reached the opener: {:?}",
        app.shell.calls()
    );
    // The plain file URL is shown in Finder, never opened.
    appcmd::open_url(&app.state, M, OpenTarget::Url(base)).unwrap();
    assert_eq!(
        app.shell.calls().last().unwrap(),
        &format!("reveal({})", script.canonicalize().unwrap().display())
    );
}

#[test]
fn add_repos_needs_an_absolute_path_to_a_folder() {
    let app = TestApp::new();
    let dir = temp_dir("repo-path");
    let file = dir.join("not-a-folder");
    std::fs::write(&file, "x").unwrap();
    for bad in [
        "relative/path".to_string(),
        file.to_string_lossy().into(),
        "/no/such/folder".into(),
    ] {
        assert_eq!(
            err_code(repos::add_repos(&app.state, M, vec![bad.clone()])),
            (
                ErrorCode::InvalidTarget,
                "Choose a folder to add as a repo.".into()
            ),
            "{bad}"
        );
    }
    // A symlink and its folder are one repo.
    let real = dir.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let alias = dir.join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let out = repos::add_repos(&app.state, M, vec![alias.to_string_lossy().into()]).unwrap();
    assert_eq!(
        out.repos[0].path,
        real.canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(out.repos[0].name, "real");
}

/// Settings › Repos (audit should-fix 8): rename, change folder or remote, and remove.
#[test]
fn a_repo_can_be_renamed_moved_and_given_another_remote() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    repo(&app, "api");
    app.sync_vm();
    let moved = temp_dir("moved").join("booking-web");
    std::fs::create_dir_all(&moved).unwrap();
    let patch = repos::edit_repo(
        &app.state,
        M,
        web,
        "booking-web".into(),
        moved.to_string_lossy().into(),
        Some("git@github.com:acme/booking-web.git".into()),
    )
    .unwrap();
    let r = app
        .core(|s| s.repos().unwrap())
        .into_iter()
        .find(|r| r.id == web)
        .unwrap();
    assert_eq!(r.name, "booking-web");
    assert_eq!(r.path, moved.canonicalize().unwrap().to_string_lossy());
    assert_eq!(
        r.remote_url.as_deref(),
        Some("git@github.com:acme/booking-web.git")
    );
    assert!(patch.repos.is_some(), "the sidebar hears the new name");
    assert_told(&app);
    // A blank remote clears it; the rest stays.
    repos::edit_repo(
        &app.state,
        M,
        web,
        r.name.clone(),
        r.path.clone(),
        Some("  ".into()),
    )
    .unwrap();
    let r = app
        .core(|s| s.repos().unwrap())
        .into_iter()
        .find(|r| r.id == web)
        .unwrap();
    assert_eq!((r.name.as_str(), r.remote_url), ("booking-web", None));
}

#[test]
fn editing_a_repo_refuses_a_blank_name_a_missing_folder_and_a_taken_one() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    repo(&app, "api");
    app.sync_vm();
    let path = "/src/web".to_string();
    assert_eq!(
        err_code(repos::edit_repo(
            &app.state,
            M,
            web,
            " ".into(),
            path.clone(),
            None
        )),
        (ErrorCode::InvalidTarget, "A repo needs a name.".into())
    );
    assert_eq!(
        err_code(repos::edit_repo(
            &app.state,
            M,
            web,
            "web".into(),
            "/no/such/folder".into(),
            None
        ))
        .0,
        ErrorCode::InvalidTarget
    );
    assert_eq!(
        err_code(repos::edit_repo(
            &app.state,
            M,
            web,
            "api".into(),
            path.clone(),
            None
        )),
        (
            ErrorCode::AlreadyExists,
            "A repo with that name or folder is already added.".into()
        )
    );
    assert_eq!(
        err_code(repos::edit_repo(
            &app.state,
            M,
            RepoId(99),
            "x".into(),
            path,
            None
        )),
        (ErrorCode::NotFound, "That repo no longer exists.".into())
    );
}

#[test]
fn removing_a_repo_unlinks_it_from_its_tasks_and_says_how_many() {
    let app = TestApp::new();
    let web = repo(&app, "web");
    let a = app.add("A", None);
    let b = app.add("B", None);
    app.add("C", None);
    app.core(|s| {
        s.link_repo(a, web, &Actor::Me).unwrap();
        s.link_repo(b, web, &Actor::Me).unwrap();
        s.set_status(b, Status::Done, None, &Actor::Me).unwrap();
    });
    app.sync_vm();
    let out = repos::remove_repo(&app.state, M, web).unwrap();
    assert_eq!(out.unlinked, vec![a, b], "done tasks lose it too");
    assert!(app.core(|s| s.repos().unwrap()).is_empty());
    assert!(
        out.patch
            .upserts
            .iter()
            .any(|r| r.id == a && r.repos.is_empty())
    );
    assert_eq!(
        err_code(repos::remove_repo(&app.state, M, web)),
        (ErrorCode::NotFound, "That repo no longer exists.".into())
    );
}

/// Settings › Database: "Back up…" through a save dialog and core's online backup.
#[test]
fn back_up_the_database_where_the_save_dialog_says() {
    let db = temp_dir("backup-src").join("airboss.db");
    let app = TestApp::with_store(airboss_core::Store::open(&db).unwrap());
    app.add("Kept", None);
    // Cancelled: nothing happens.
    assert_eq!(repos::back_up_database(&app.state, M).unwrap(), None);
    let to = temp_dir("backup-dst").join("Airboss backup.db");
    app.shell.answer_save(Some(to.clone()));
    let out = repos::back_up_database(&app.state, M).unwrap();
    assert_eq!(out.as_deref(), Some(to.to_string_lossy().as_ref()));
    let copy = airboss_core::Store::open(&to).unwrap();
    assert_eq!(copy.tree().unwrap()[0].task.title, "Kept");
    // The dialog already asked about replacing, so a second backup there overwrites.
    repos::back_up_database(&app.state, M).unwrap();
    assert!(
        app.shell
            .calls()
            .iter()
            .any(|c| c.starts_with("pick_save(Airboss backup "))
    );
}

#[test]
fn install_cli_never_replaces_someone_elses_link() {
    let app = TestApp::new();
    let sidecar = temp_dir("bundle2").join("airboss");
    std::fs::write(&sidecar, b"#!/bin/sh\n").unwrap();
    app.shell.set_sidecar(Some(sidecar.clone()));
    let bin = app.home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink("/opt/elsewhere/airboss", bin.join("airboss")).unwrap();
    let (code, message) = err_code(appcmd::install_cli(&app.state, M));
    assert_eq!(code, ErrorCode::AlreadyExists);
    assert!(message.contains("/opt/elsewhere/airboss"), "{message}");
    assert_eq!(
        std::fs::read_link(bin.join("airboss")).unwrap(),
        Path::new("/opt/elsewhere/airboss")
    );
    // A link left by a copy of the app that moved is ours to replace.
    std::fs::remove_file(bin.join("airboss")).unwrap();
    std::os::unix::fs::symlink(
        "/Volumes/Old/Airboss.app/Contents/MacOS/airboss",
        bin.join("airboss"),
    )
    .unwrap();
    appcmd::install_cli(&app.state, M).unwrap();
    assert_eq!(std::fs::read_link(bin.join("airboss")).unwrap(), sidecar);
}

#[test]
fn report_a_problem_opens_the_bug_form_with_the_versions_filled_in() {
    assert_eq!(
        appcmd::report_problem_url("0.1.0-alpha.1", "15.2"),
        "https://github.com/amantibrewal310/airboss/issues/new?template=bug.yml&version=0.1.0-alpha.1&macos=15.2"
    );
    // Anything outside a query value's plain characters is escaped.
    assert_eq!(
        appcmd::report_problem_url("1 & 2", ""),
        "https://github.com/amantibrewal310/airboss/issues/new?template=bug.yml&version=1%20%26%202&macos="
    );
    // It goes through the link allowlist and the checked browser, like any https link.
    let app = TestApp::new();
    appcmd::report_problem(&app.state, M).unwrap();
    let form = format!(
        "open_url(https://github.com/amantibrewal310/airboss/issues/new?template=bug.yml&version={}&macos=",
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        app.shell.calls().iter().any(|c| c.starts_with(&form)),
        "{:?}",
        app.shell.calls()
    );
}
