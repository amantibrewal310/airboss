//! Every type that crosses the IPC boundary survives JSON, in the shape the contract names:
//! enums as snake_case strings, `PrRef`, `NeedsKey` and `Timestamp` as plain strings.
#![allow(clippy::unwrap_used)]

use std::fmt::Debug;
use std::path::PathBuf;

use airboss_core::{PrState, Repo, RepoId, TaskId};
use airboss_services::{
    Availability, Checks, Chip, Connection, ErrorCode, LaunchReport, Mode, NeedsGroup, NeedsItem,
    NeedsKey, NeedsKind, NeedsList, Place, PrProblem, PrRef, PrView, Prepared, Refresh, Request,
    RequestKind, Review, ServiceError, Source, StartRequest, StartedThread, T3Project, Target,
    ThreadKey, ThreadState, ThreadView,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Serialises, reads back, and checks the value is unchanged; returns the JSON.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, value, "{json}");
    json
}

fn key() -> ThreadKey {
    ThreadKey {
        source: Source::T3,
        r#ref: "97b5c8b5".into(),
    }
}

fn pr_ref() -> PrRef {
    PrRef::parse("acme/web#42").unwrap()
}

fn error() -> ServiceError {
    ServiceError::new(
        ErrorCode::NotAProject,
        "booking-api isn't a T3 project yet.",
    )
}

fn repo() -> Repo {
    Repo {
        id: RepoId(3),
        name: "booking-api".into(),
        path: "/src/booking-api".into(),
        remote_url: Some("git@github.com:acme/booking-api.git".into()),
    }
}

fn started() -> StartedThread {
    StartedThread {
        key: key(),
        mode: Mode::DoIt,
        repo: Some(RepoId(3)),
        title: "Booking flow (booking-api)".into(),
        linked: true,
    }
}

fn item() -> NeedsItem {
    NeedsItem {
        key: NeedsKey("question:req-1".into()),
        kind: NeedsKind::Question,
        group: NeedsGroup::Questions,
        task: TaskId(142),
        task_title: "Booking flow".into(),
        since: "2026-11-26T09:00:00.000Z".into(),
        headline: "Which payment provider?".into(),
        context: "Agent in Booking flow asks".into(),
        target: Target::Thread(key()),
        proposed: vec![(TaskId(150), "Add retries".into())],
        stale: false,
    }
}

#[test]
fn enums_are_snake_case_strings() {
    let cases: Vec<(String, &str)> = vec![
        (
            serde_json::to_string(&ErrorCode::T3NotRunning).unwrap(),
            "\"t3_not_running\"",
        ),
        (
            serde_json::to_string(&ErrorCode::NotAProject).unwrap(),
            "\"not_a_project\"",
        ),
        (
            serde_json::to_string(&ThreadState::NeedsMe).unwrap(),
            "\"needs_me\"",
        ),
        (
            serde_json::to_string(&Chip::WaitingOnYou).unwrap(),
            "\"waiting_on_you\"",
        ),
        (serde_json::to_string(&Source::T3).unwrap(), "\"t3\""),
        (
            serde_json::to_string(&Review::ChangesRequested).unwrap(),
            "\"changes_requested\"",
        ),
        (
            serde_json::to_string(&NeedsKind::FailedRun).unwrap(),
            "\"failed_run\"",
        ),
        (
            serde_json::to_string(&NeedsGroup::FinishIt).unwrap(),
            "\"finish_it\"",
        ),
        (serde_json::to_string(&Mode::PlanIt).unwrap(), "\"plan_it\""),
        (
            serde_json::to_string(&Availability::Unknown).unwrap(),
            "\"unknown\"",
        ),
    ];
    for (got, want) in cases {
        assert_eq!(got, want);
    }
}

#[test]
fn ids_and_keys_are_plain_strings_and_refs_named_ref() {
    assert_eq!(round_trip(&pr_ref()), "\"acme/web#42\"");
    assert_eq!(round_trip(&NeedsKey("ready:7:x".into())), "\"ready:7:x\"");
    assert_eq!(round_trip(&key()), r#"{"source":"t3","ref":"97b5c8b5"}"#);
    let target = Target::Pr {
        r#ref: pr_ref(),
        url: None,
    };
    assert_eq!(
        round_trip(&target),
        r#"{"pr":{"ref":"acme/web#42","url":null}}"#
    );
}

#[test]
fn a_pr_ref_is_checked_when_read() {
    // Not canonical yet: read through the same rule as `core`.
    let read: PrRef = serde_json::from_str("\"Acme/Web#42\"").unwrap();
    assert_eq!(read, pr_ref());
    assert!(serde_json::from_str::<PrRef>("\"not a ref\"").is_err());
    assert!(serde_json::from_str::<PrRef>("\"acme/web#0\"").is_err());
}

#[test]
fn every_error_code_round_trips() {
    use ErrorCode::*;
    for code in [
        T3Missing,
        T3NotRunning,
        T3Unreachable,
        T3TooOld,
        Timeout,
        NotAProject,
        RepoMissing,
        NoBaseBranch,
        BadBranch,
        TaskClosed,
        TaskProposed,
        NeedsChoice,
        Waiting,
        AlreadyStarting,
        BadAnswer,
        T3Failed,
        GhMissing,
        SignedOut,
        NoAccess,
        PrNotFound,
        BadPrRef,
        RateLimited,
        GhFailed,
        T3Unavailable,
        NotFound,
        Core,
        Storage,
        Internal,
        ShuttingDown,
    ] {
        round_trip(&ServiceError::new(code, "x"));
    }
}

#[test]
fn connections_round_trip() {
    round_trip(&Connection {
        name: "GitHub".into(),
        ok: false,
        state: "Signed out".into(),
        detail: "Pull requests sync every 5 minutes.".into(),
        fix: Some("Run `gh auth login` in a terminal.".into()),
    });
}

#[test]
fn sync_values_round_trip() {
    for state in [
        ThreadState::Running,
        ThreadState::NeedsMe,
        ThreadState::Errored,
        ThreadState::Idle,
        ThreadState::Settled,
        ThreadState::Gone,
    ] {
        round_trip(&state);
        round_trip(&state.chip());
    }
    round_trip(&ThreadView {
        key: key(),
        linked: true,
        state: ThreadState::NeedsMe,
        title: Some("Booking flow".into()),
        request: Some(Request {
            id: "req-1".into(),
            kind: RequestKind::Approval,
            text: "Run npm install".into(),
            at: "2026-11-26T09:00:00.000Z".into(),
        }),
        error: None,
        run_id: Some("run-3".into()),
        project_path: Some("/src/booking-api".into()),
        branch: Some("task-142-booking-flow".into()),
        worktree_path: Some("/src/.t3/worktrees/x".into()),
        parent_ref: None,
        child_count: 2,
        last_activity_at: Some("2026-11-26T09:00:00.000Z".into()),
        stale: true,
    });
    for problem in [
        None,
        Some(PrProblem::NotFound),
        Some(PrProblem::NoAccess),
        Some(PrProblem::Failed("gh: HTTP 502".into())),
    ] {
        round_trip(&PrView {
            r#ref: pr_ref(),
            linked: true,
            url: Some("https://github.com/acme/web/pull/42".into()),
            title: Some("Booking page".into()),
            state: Some(PrState::Open),
            draft: false,
            review: Review::Required,
            checks: Checks::Failing,
            additions: Some(412),
            deletions: Some(38),
            head_ref: Some("task-142-booking-flow".into()),
            author: Some("aman".into()),
            updated_at: Some("2026-11-26T09:00:00Z".into()),
            ready_since: Some("2026-11-25T09:00:00.000Z".into()),
            problem,
            stale: false,
        });
    }
    for review in [
        Review::Approved,
        Review::ChangesRequested,
        Review::Required,
        Review::None,
    ] {
        round_trip(&review);
    }
    for checks in [
        Checks::Passing,
        Checks::Failing,
        Checks::Pending,
        Checks::None,
    ] {
        round_trip(&checks);
    }
    for refresh in [
        Refresh::All,
        Refresh::Prs,
        Refresh::PrsOf(TaskId(4)),
        Refresh::T3,
    ] {
        round_trip(&refresh);
    }
    round_trip(&T3Project {
        id: "p1".into(),
        name: "booking-api".into(),
        path: PathBuf::from("/src/booking-api"),
    });
    round_trip(&ThreadKey {
        source: Source::Claude,
        r#ref: "sess-1".into(),
    });
}

#[test]
fn launch_values_round_trip() {
    for availability in [
        Availability::Unknown,
        Availability::Ready {
            version: "t3 v0.9.2".into(),
        },
        Availability::Missing {
            reason: "Install T3 Code.".into(),
        },
    ] {
        round_trip(&availability);
    }
    for place in [
        Place::TaskRepos,
        Place::NoRepo(PathBuf::from("/tmp/research")),
    ] {
        round_trip(&StartRequest {
            task: TaskId(142),
            mode: Mode::PlanIt,
            place,
            again: true,
        });
    }
    round_trip(&Prepared {
        repos: vec![repo()],
        from_subtasks: true,
        needs_choice: false,
        choices: vec![repo()],
        branch: Some("task-142-booking-flow".into()),
        research_dir: PathBuf::from("/Users/me/.t3/scratch"),
        waiting_on: vec![TaskId(143), TaskId(145)],
        started: vec![started()],
    });
    round_trip(&LaunchReport {
        started: vec![started()],
        skipped: vec![RepoId(4)],
        failed: vec![(Some(RepoId(5)), error()), (None, error())],
        branch: Some("task-142-booking-flow".into()),
        moved_to_doing: true,
    });
}

#[test]
fn attention_values_round_trip() {
    let mut items = vec![item()];
    for target in [
        Target::Pr {
            r#ref: pr_ref(),
            url: Some("https://github.com/acme/web/pull/42".into()),
        },
        Target::Task(TaskId(7)),
    ] {
        items.push(NeedsItem { target, ..item() });
    }
    round_trip(&NeedsList {
        count: 3,
        items,
        checked_at: Some("2026-11-26T09:00:00.000Z".into()),
        version: 12,
    });
    round_trip(&NeedsList::default());
    use NeedsKind::*;
    for kind in [
        Question, Approval, Review, FailedRun, Proposed, Ready, FinishIt, Quiet,
    ] {
        round_trip(&kind);
        round_trip(&kind.group());
    }
}
