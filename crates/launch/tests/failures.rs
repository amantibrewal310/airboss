//! Every failure: its code, its row, whether it stops the loop, and partial failures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use airboss_core::{Actor, NewTask, Status};
use airboss_launch::fake::Reply;
use airboss_launch::{CoreWrite, FailWith, LaunchMsg, T3_MISSING};
use airboss_services::{ErrorCode, LaunchService, Mode, Place, StartRequest};
use common::{H, again, do_it, ids, plan_it};

fn exit(line: &str) -> Reply {
    Reply::Exit(1, line.into())
}

/// One repo, one scripted answer: the error code, and the row's state.
fn one(reply: Reply) -> (ErrorCode, String, String) {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.t3.push(reply);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert!(report.started.is_empty());
    assert_eq!(report.failed.len(), 1);
    let (repo, e) = &report.failed[0];
    assert_eq!(*repo, Some(web.id));
    // Nothing started, so the task stays where it was.
    assert_eq!(h.get(t.id).status, Status::Todo);
    assert!(!report.moved_to_doing);
    (e.code, e.message.clone(), h.rows(t.id)[0].2.clone())
}

#[test]
fn each_t3_error_maps_to_its_code_and_row() {
    let cases = [
        (
            exit("No running T3 Code server uses /x"),
            ErrorCode::T3NotRunning,
            "failed",
        ),
        (
            exit("Could not reach the T3 Code server: refused"),
            ErrorCode::T3Unreachable,
            "failed",
        ),
        (
            exit("This T3 Code server has no t3_thread_launch"),
            ErrorCode::T3TooOld,
            "failed",
        ),
        (
            exit("No T3 Code project matches /x"),
            ErrorCode::NotAProject,
            "failed",
        ),
        (exit("Something odd"), ErrorCode::T3Failed, "failed"),
        (
            Reply::Exit(127, "T3 Code has moved or been removed.".into()),
            ErrorCode::T3Missing,
            "failed",
        ),
        (Reply::Missing, ErrorCode::T3Missing, "failed"),
        (Reply::Timeout, ErrorCode::Timeout, "pending"),
        (
            Reply::Stdout("not json".into()),
            ErrorCode::BadAnswer,
            "pending",
        ),
        (Reply::Killed, ErrorCode::BadAnswer, "pending"),
    ];
    for (reply, code, state) in cases {
        let (got, message, row) = one(reply);
        assert_eq!((got, row.as_str()), (code, state), "{message}");
    }
}

#[test]
fn the_messages_name_the_repo() {
    let (_, m, _) = one(exit("No T3 Code project matches /x"));
    assert_eq!(m, "web isn't a T3 project yet.");
    let (_, m, _) = one(exit("No running T3 Code server uses /x"));
    assert_eq!(m, "T3 Code isn't open. Open it, then try again.");
    let (_, m, _) = one(Reply::Missing);
    assert_eq!(m, T3_MISSING);
}

#[test]
fn an_error_that_means_t3_cant_start_anything_stops_the_loop() {
    for reply in [
        exit("No running T3 Code server uses /x"),
        Reply::Missing,
        Reply::Timeout,
        exit("This T3 Code server has no t3_thread_launch"),
    ] {
        let h = H::new();
        let (a, b, c) = (h.repo("a"), h.repo("b"), h.repo("c"));
        let t = h.task("T", &[&a, &b, &c]);
        h.t3.push(reply);
        let report = h.launch.start(&do_it(t.id)).unwrap();
        assert_eq!(
            h.t3.starts().len(),
            1,
            "t3 isn't tried again after a global error"
        );
        let failed: Vec<_> = report.failed.iter().map(|(r, _)| *r).collect();
        assert_eq!(failed, vec![Some(a.id), Some(b.id), Some(c.id)]);
        let codes: Vec<_> = report.failed.iter().map(|(_, e)| e.code).collect();
        assert!(codes.windows(2).all(|w| w[0] == w[1]), "{codes:?}");
        // Only the place that ran has a row.
        assert_eq!(h.rows(t.id).len(), 1);
    }
}

#[test]
fn repo_2_of_3_failing_still_starts_1_and_3_and_a_retry_starts_only_2() {
    let h = H::new();
    let (a, b, c) = (h.repo("a"), h.repo("b"), h.repo("c"));
    let t = h.task("Three repos", &[&a, &b, &c]);
    h.t3.push(Reply::Thread("thread-a".into()));
    h.t3.push(exit("No T3 Code project matches /b"));
    h.t3.push(Reply::Thread("thread-c".into()));
    let report = h.launch.start(&do_it(t.id)).unwrap();
    let started: Vec<_> = report.started.iter().map(|s| s.repo).collect();
    assert_eq!(started, vec![Some(a.id), Some(c.id)]);
    assert!(report.started.iter().all(|s| s.linked));
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, Some(b.id));
    assert_eq!(report.failed[0].1.code, ErrorCode::NotAProject);
    assert!(report.moved_to_doing);
    assert_eq!(h.get(t.id).status, Status::Doing);
    let linked: Vec<String> = h
        .store
        .lock()
        .unwrap()
        .links(t.id)
        .unwrap()
        .into_iter()
        .map(|l| l.r#ref)
        .collect();
    assert_eq!(linked, ["thread-a", "thread-c"]);

    // "Retry" sends the same request: only b runs.
    let retry = h.launch.start(&do_it(t.id)).unwrap();
    let starts = h.t3.starts();
    assert_eq!(starts.len(), 4);
    assert_eq!(starts[3].project(), Some(b.path.as_str()));
    assert_eq!(retry.skipped, ids(&[&a, &c]));
    assert_eq!(retry.started.len(), 1);
    assert_eq!(retry.started[0].repo, Some(b.id));
    assert!(retry.failed.is_empty());
    // Already doing: InvalidTransition counts as success.
    assert!(!retry.moved_to_doing);
    assert_eq!(h.store.lock().unwrap().links(t.id).unwrap().len(), 3);
}

#[test]
fn a_failed_link_is_reported_unlinked() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core.fail_next(CoreWrite::Link, FailWith::Db);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(!report.started[0].linked);
    assert!(h.store.lock().unwrap().links(t.id).unwrap().is_empty());
    // It still counts as started.
    assert!(report.moved_to_doing);
    let prepared = h.launch.prepare(t.id, Mode::DoIt).unwrap();
    assert!(!prepared.started[0].linked);
}

#[test]
fn invalid_transition_on_the_move_to_doing_is_success() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core
        .fail_next(CoreWrite::SetStatus, FailWith::InvalidTransition);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(report.failed.is_empty());
    assert!(!report.moved_to_doing);
}

#[test]
fn any_other_failure_on_the_move_still_counts_the_threads() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core.fail_next(CoreWrite::SetStatus, FailWith::Db);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(report.started[0].linked);
    assert!(!report.moved_to_doing);
}

#[test]
fn a_task_already_doing_or_in_review_is_left_where_it_is() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.store
        .lock()
        .unwrap()
        .set_status(t.id, Status::InReview, None, &Actor::Me)
        .unwrap();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.started.len(), 1);
    assert!(!report.moved_to_doing);
    assert_eq!(h.get(t.id).status, Status::InReview);
}

#[test]
fn a_blocked_task_moves_to_doing() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.store
        .lock()
        .unwrap()
        .set_status(t.id, Status::Blocked, Some("waiting on design"), &Actor::Me)
        .unwrap();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert!(report.moved_to_doing);
    assert_eq!(h.get(t.id).status, Status::Doing);
}

#[test]
fn a_failed_set_branch_starts_nothing() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.core.fail_next(CoreWrite::SetBranch, FailWith::Db);
    let e = h.launch.start(&do_it(t.id)).unwrap_err();
    assert_eq!(e.code, ErrorCode::Core);
    assert!(h.t3.calls().is_empty());
    assert!(h.rows(t.id).is_empty());
    assert_eq!(h.get(t.id).branch, None);
}

#[test]
fn a_waiting_task_is_refused_for_do_it_but_can_be_planned() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    let a = h.task("A", &[]);
    let b = h.task("B", &[]);
    {
        let mut s = h.store.lock().unwrap();
        s.add_dependency(t.id, a.id, &Actor::Me).unwrap();
        s.add_dependency(t.id, b.id, &Actor::Me).unwrap();
    }
    let e = h.launch.start(&do_it(t.id)).unwrap_err();
    assert_eq!(e.code, ErrorCode::Waiting);
    assert_eq!(
        e.message,
        format!("Task {} waits on {} and {}.", t.id.0, a.id.0, b.id.0)
    );
    // Research only is Do it too.
    let req = StartRequest {
        task: t.id,
        mode: Mode::DoIt,
        place: Place::NoRepo(h.cfg.research_dir.clone()),
        again: false,
        launcher: airboss_services::Launcher::T3,
    };
    assert_eq!(h.launch.start(&req).unwrap_err().code, ErrorCode::Waiting);
    assert!(h.t3.calls().is_empty());
    assert_eq!(h.launch.start(&plan_it(t.id)).unwrap().started.len(), 1);
}

#[test]
fn closed_proposed_and_missing_tasks_are_refused() {
    let h = H::new();
    let web = h.repo("web");
    let mut s = h.store.lock().unwrap();
    let proposed = s
        .add_task(
            NewTask::new("Idea").in_repo(web.id),
            &Actor::Agent("t3/x".into()),
        )
        .unwrap();
    let done = s
        .add_task(NewTask::new("Done").in_repo(web.id), &Actor::Me)
        .unwrap();
    s.set_status(done.id, Status::Done, None, &Actor::Me)
        .unwrap();
    drop(s);
    assert_eq!(
        h.launch.start(&do_it(proposed.id)).unwrap_err().code,
        ErrorCode::TaskProposed
    );
    assert_eq!(
        h.launch.start(&plan_it(done.id)).unwrap_err().code,
        ErrorCode::TaskClosed
    );
    let gone = airboss_core::TaskId(4242);
    assert_eq!(
        h.launch.start(&do_it(gone)).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(h.t3.calls().is_empty());
}

#[test]
fn a_task_with_no_repo_needs_a_choice() {
    let h = H::new();
    let t = h.task("Where?", &[]);
    let e = h.launch.start(&do_it(t.id)).unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsChoice);
    assert!(h.t3.calls().is_empty());
    assert_eq!(h.get(t.id).branch, None);
}

#[test]
fn a_missing_folder_writes_no_row() {
    let h = H::new();
    let web = h.repo("web");
    let api = h.repo("api");
    std::fs::remove_dir_all(&web.path).unwrap();
    let t = h.task("T", &[&web, &api]);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.failed.len(), 1);
    let (repo, e) = &report.failed[0];
    assert_eq!((*repo, e.code), (Some(web.id), ErrorCode::RepoMissing));
    assert_eq!(e.message, format!("web isn't at {} any more.", web.path));
    assert_eq!(report.started.len(), 1);
    assert_eq!(h.rows(t.id).len(), 1, "only api's row");
}

#[test]
fn a_research_folder_must_be_a_full_existing_path() {
    let h = H::new();
    let t = h.task("Research", &[]);
    for dir in [
        PathBuf::from("relative/dir"),
        h.dir.path().join("nope"),
        PathBuf::from("-rf"),
    ] {
        let req = StartRequest {
            task: t.id,
            mode: Mode::DoIt,
            place: Place::NoRepo(dir.clone()),
            again: false,
            launcher: airboss_services::Launcher::T3,
        };
        let report = h.launch.start(&req).unwrap();
        assert_eq!(report.failed.len(), 1, "{dir:?}");
        assert_eq!(report.failed[0], (None, report.failed[0].1.clone()));
        assert_eq!(report.failed[0].1.code, ErrorCode::RepoMissing);
    }
    assert!(h.t3.calls().is_empty());
    assert!(h.rows(t.id).is_empty());
}

#[test]
fn no_base_branch_writes_no_row() {
    let h = H::new();
    let web = h.repo("web");
    h.git.set_default(h.path_of(&web), None);
    let t = h.task("T", &[&web]);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.failed[0].1.code, ErrorCode::NoBaseBranch);
    assert_eq!(
        report.failed[0].1.message,
        "Couldn't find web's main branch. Set one in the repo's settings."
    );
    assert!(h.rows(t.id).is_empty());
    assert!(h.t3.calls().is_empty());
}

#[test]
fn a_base_that_could_be_read_as_a_flag_is_refused() {
    let h = H::new();
    let web = h.repo("web");
    h.git
        .set_default(h.path_of(&web), Some("--upload-pack=evil"));
    let t = h.task("T", &[&web]);
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(report.failed[0].1.code, ErrorCode::NoBaseBranch);
    assert!(h.t3.calls().is_empty());
}

#[test]
fn a_branch_git_refuses_starts_nothing() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    // Core allows `//`; git doesn't.
    h.store
        .lock()
        .unwrap()
        .set_branch(t.id, Some("feature//x"), &Actor::Me)
        .unwrap();
    let e = h.launch.start(&do_it(t.id)).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadBranch);
    assert!(h.t3.calls().is_empty());
    assert!(h.rows(t.id).is_empty());
}

#[test]
fn a_bad_again_branch_fails_only_its_place() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("Booking flow", &[&web]);
    h.launch.start(&do_it(t.id)).unwrap();
    h.git
        .refuse_name(&format!("task-{}-booking-flow-2", t.id.0));
    let report = h.launch.start(&again(do_it(t.id))).unwrap();
    assert_eq!(report.failed[0].1.code, ErrorCode::BadBranch);
    assert_eq!(h.t3.starts().len(), 1);
}

#[test]
fn t3_missing_queues_an_availability_check() {
    let mut h = H::new();
    while h.worker.try_next().is_some() {}
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    h.t3.push(Reply::Missing);
    h.launch.start(&do_it(t.id)).unwrap();
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::CheckAvailability));
}

#[test]
fn a_timeout_says_the_thread_may_still_appear() {
    let (code, message, row) = one(Reply::Timeout);
    assert_eq!(code, ErrorCode::Timeout);
    assert_eq!(
        message,
        "T3 didn't answer in 30 s. The thread may still appear; Airboss will link it if it does."
    );
    assert_eq!(row, "pending");
}

#[test]
fn failing_to_record_the_t3_project_still_counts_the_thread() {
    let h = H::new();
    let web = h.repo("web");
    let t = h.task("T", &[&web]);
    // Every write to launch_repos fails from now on.
    h.db()
        .execute_batch(
            "CREATE TRIGGER no_projects BEFORE INSERT ON launch_repos
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    let report = h.launch.start(&do_it(t.id)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(report.started.len(), 1);
    assert!(report.started[0].linked);
    assert!(report.moved_to_doing);
    assert_eq!(h.get(t.id).status, Status::Doing);
    assert_eq!(h.store.lock().unwrap().links(t.id).unwrap().len(), 1);
}
