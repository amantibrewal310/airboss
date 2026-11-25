//! `T3Db` against T3's real schema in a temp file: the checks, and Q0–Q8.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use airboss_sync::fake::{FIXTURE_ENVIRONMENT, T3Fixture, TempDir};
use airboss_sync::t3::{
    RunStatus, T3Db, T3Error, T3Query, T3Read, T3RequestItem, T3RequestKind, T3Source,
};

const LINKED: &[&str] = &[
    "t-run",
    "t-queued",
    "t-question",
    "t-approve",
    "t-approve2",
    "t-root",
    "t-settled",
    "t-errored",
    "t-failed",
    "t-idle",
    "t-noruns",
    "t-archived",
    "t-deleted",
    "t-prs",
    "t-missing",
];

fn query() -> T3Query {
    T3Query {
        linked: LINKED.iter().map(|s| s.to_string()).collect(),
        untracked_since: Some("2026-11-19T12:00:00.000Z".into()),
        ..T3Query::default()
    }
}

fn open(f: &T3Fixture) -> T3Db {
    let mut db = T3Db::new(f.dir());
    db.open().unwrap();
    db
}

fn read(f: &T3Fixture) -> T3Read {
    open(f).read(&query()).unwrap()
}

fn broken(f: &T3Fixture) -> String {
    let mut db = T3Db::new(f.dir());
    let err = match db.open() {
        Err(e) => e,
        Ok(()) => db.read(&query()).expect_err("should be broken"),
    };
    match err {
        T3Error::Broken(why) => why,
        other => panic!("expected Broken, got {other:?}"),
    }
}

#[test]
fn no_database_is_missing() {
    let dir = TempDir::new();
    assert_eq!(T3Db::new(dir.path()).open(), Err(T3Error::Missing));
}

#[test]
fn the_fixture_passes_every_check() {
    let f = T3Fixture::with_rows();
    let mut db = open(&f);
    assert_eq!(
        db.environment_id().unwrap().as_deref(),
        Some(FIXTURE_ENVIRONMENT)
    );
    let v1 = db.data_version().unwrap();
    f.exec("UPDATE orchestration_v2_projection_threads SET title = 'x' WHERE thread_id = 't-run'");
    assert_ne!(
        db.data_version().unwrap(),
        v1,
        "a T3 commit moves data_version"
    );
}

#[test]
fn the_connection_is_read_only() {
    let f = T3Fixture::with_rows();
    let mut db = open(&f);
    db.read(&query()).unwrap();
    // Nothing T3Db holds can write; prove the file is untouched by our side.
    let before = std::fs::metadata(f.dir().join("statev2.sqlite"))
        .unwrap()
        .modified()
        .unwrap();
    db.read(&query()).unwrap();
    let after = std::fs::metadata(f.dir().join("statev2.sqlite"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn q1_reads_linked_threads_and_skips_unknown_ids() {
    let r = read(&T3Fixture::with_rows());
    let ids: Vec<&str> = r.threads.iter().map(|t| t.id.as_str()).collect();
    assert!(!ids.contains(&"t-missing"));
    assert!(!ids.contains(&"t-child"), "only linked ids are read");
    assert_eq!(ids.len(), LINKED.len() - 1);
    let run = r.threads.iter().find(|t| t.id == "t-run").unwrap();
    assert_eq!(run.branch.as_deref(), Some("feat/run"));
    assert_eq!(run.worktree_path.as_deref(), Some("/work/wt/run"));
    assert_eq!(run.project_path.as_deref(), Some("/work/web"));
    assert_eq!(run.updated_at, "2026-11-26T11:00:00.000Z");
    assert!(!run.gone);
    let gone: Vec<&str> = r
        .threads
        .iter()
        .filter(|t| t.gone)
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(gone, vec!["t-archived", "t-deleted"]);
    let settled = r.threads.iter().find(|t| t.id == "t-settled").unwrap();
    assert_eq!(
        settled.settled_at.as_deref(),
        Some("2026-11-25T12:00:00.000Z")
    );
}

#[test]
fn json_null_is_accepted() {
    let r = read(&T3Fixture::with_rows());
    let idle = r.threads.iter().find(|t| t.id == "t-idle").unwrap();
    assert_eq!((idle.branch.as_ref(), idle.parent.as_ref()), (None, None));
    assert!(
        idle.pull_requests.is_empty(),
        "no $.pullRequests key means no PRs"
    );
}

#[test]
fn thread_prs_are_read_with_every_key() {
    let r = read(&T3Fixture::with_rows());
    let t = r.threads.iter().find(|t| t.id == "t-prs").unwrap();
    assert_eq!(t.pull_requests.len(), 3);
    let first = &t.pull_requests[0];
    assert_eq!(first.host.as_deref(), Some("github.com"));
    assert_eq!(first.repository.as_deref(), Some("Acme/Web"));
    assert_eq!(first.number, Some(7));
    assert_eq!(first.linked_at.as_deref(), Some("2026-11-26T10:00:00.000Z"));
}

#[test]
fn q2_reads_only_the_latest_run() {
    let r = read(&T3Fixture::with_rows());
    let run = |t: &str| r.runs.iter().find(|x| x.thread_id == t);
    assert_eq!(run("t-run").unwrap().run_id, "r-run-2");
    assert_eq!(run("t-run").unwrap().status, RunStatus::Running);
    assert_eq!(run("t-errored").unwrap().run_id, "r-errored-2");
    assert_eq!(run("t-queued").unwrap().started_at, None);
    assert!(run("t-noruns").is_none());
    assert_eq!(
        run("t-idle").unwrap().completed_at.as_deref(),
        Some("2026-11-26T05:30:00.000Z")
    );
}

#[test]
fn q3_reads_every_pending_request_oldest_first() {
    let r = read(&T3Fixture::with_rows());
    let ids: Vec<&str> = r.requests.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["q-child", "q-approve2", "q-approve", "q-question"]
    );
    let q = |id: &str| r.requests.iter().find(|q| q.id == id).unwrap();
    assert_eq!(q("q-question").kind, T3RequestKind::UserInput);
    assert_eq!(
        q("q-question").item,
        Some(T3RequestItem::Question("Ship it to staging now?".into()))
    );
    assert_eq!(
        q("q-approve").item,
        Some(T3RequestItem::Approval(Some(
            "cargo publish --dry-run".into()
        )))
    );
    assert_eq!(q("q-approve2").item, Some(T3RequestItem::Approval(None)));
    assert_eq!(q("q-child").root.as_deref(), Some("t-root"));
}

#[test]
fn q4_reads_the_newest_failure_of_new_failed_runs_only() {
    let f = T3Fixture::with_rows();
    let r = read(&f);
    assert_eq!(
        r.failures.get("r-errored-2"),
        Some(&Some(
            "Usage limit reached. Try again at 18:00.".to_string()
        ))
    );
    assert_eq!(r.failures.get("r-failed-1"), Some(&None), "no error item");
    assert!(!r.failures.contains_key("r-run-1"), "not the latest run");
    let mut q = query();
    q.known_failures.insert("r-errored-2".into());
    let again = open(&f).read(&q).unwrap();
    assert!(!again.failures.contains_key("r-errored-2"));
}

#[test]
fn q5_counts_children_of_linked_roots() {
    let r = read(&T3Fixture::with_rows());
    assert_eq!(r.children.get("t-root"), Some(&2));
    assert_eq!(
        r.children.get("t-run"),
        None,
        "a thread counts as its own root, not a child"
    );
}

#[test]
fn q6_lists_recent_top_level_threads_and_q8_live_projects() {
    let f = T3Fixture::with_rows();
    let r = read(&f);
    let ids: Vec<String> = r.untracked.unwrap().into_iter().map(|t| t.id).collect();
    assert!(
        ids.contains(&"t-run".to_string()),
        "linked ones too; sync filters them"
    );
    for excluded in [
        "t-child",
        "t-child2",
        "t-side",
        "t-old",
        "t-archived",
        "t-deleted",
    ] {
        assert!(!ids.contains(&excluded.to_string()), "{excluded}");
    }
    assert_eq!(ids[0], "t-question", "newest first");
    let projects: Vec<String> = r.projects.unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(projects, vec!["web", "ui", "api"]);

    let mut q = query();
    q.untracked_since = None;
    let r = open(&f).read(&q).unwrap();
    assert_eq!((r.untracked, r.projects), (None, None));
}

#[test]
fn q7_finds_the_newest_live_thread_in_the_deepest_project() {
    let f = T3Fixture::with_rows();
    let mut db = open(&f);
    let find = |db: &mut T3Db, p: &str, b: &str| db.find_thread(Path::new(p), b).unwrap();
    assert_eq!(
        find(&mut db, "/work/web", "feat/run").as_deref(),
        Some("t-web-new")
    );
    assert_eq!(
        find(&mut db, "/work/web/src", "feat/run").as_deref(),
        Some("t-web-new")
    );
    assert_eq!(
        find(&mut db, "/work/web/packages/ui", "feat/run").as_deref(),
        Some("t-ui")
    );
    assert_eq!(find(&mut db, "/work/web", "feat/nope"), None);
    assert_eq!(
        find(&mut db, "/work/gone", "feat/run"),
        None,
        "deleted project"
    );
    assert_eq!(find(&mut db, "/elsewhere", "feat/run"), None);
}

#[test]
fn a_missing_column_is_broken() {
    let f = T3Fixture::with_rows();
    f.exec(
        "DROP INDEX orchestration_v2_projection_runs_last_executed_idx;
         ALTER TABLE orchestration_v2_projection_runs DROP COLUMN completed_at;",
    );
    let why = broken(&f);
    assert!(why.contains("orchestration_v2_projection_runs"), "{why}");
    assert!(why.contains("completed_at"), "{why}");
}

#[test]
fn a_missing_table_is_broken() {
    let f = T3Fixture::with_rows();
    f.exec("DROP TABLE projection_projects");
    assert!(broken(&f).contains("projection_projects"));
}

#[test]
fn an_unknown_run_status_is_broken() {
    let f = T3Fixture::with_rows();
    f.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'paused' WHERE run_id = 'r-idle-1'",
    );
    assert!(broken(&f).contains("\"paused\""));
}

#[test]
fn an_unknown_request_kind_or_status_is_broken() {
    let f = T3Fixture::with_rows();
    f.exec(
        "UPDATE orchestration_v2_projection_runtime_requests SET kind = 'telepathy'
         WHERE runtime_request_id = 'q-old'",
    );
    assert!(broken(&f).contains("telepathy"));
    let f = T3Fixture::with_rows();
    f.exec(
        "UPDATE orchestration_v2_projection_runtime_requests SET status = 'snoozed'
         WHERE runtime_request_id = 'q-old'",
    );
    assert!(broken(&f).contains("snoozed"));
}

#[test]
fn a_value_that_appears_after_open_is_broken_on_the_next_read() {
    let f = T3Fixture::with_rows();
    let mut db = open(&f);
    f.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'paused' WHERE run_id = 'r-run-2'",
    );
    assert!(matches!(db.read(&query()), Err(T3Error::Broken(w)) if w.contains("paused")));
}

#[test]
fn a_missing_required_key_is_broken() {
    for (key, sql) in [
        (
            "$.settledAt",
            "UPDATE orchestration_v2_projection_threads SET payload_json = json_remove(payload_json, '$.settledAt') WHERE thread_id = 't-idle'",
        ),
        (
            "$.lineage.rootThreadId",
            "UPDATE orchestration_v2_projection_threads SET payload_json = json_remove(payload_json, '$.lineage.rootThreadId') WHERE thread_id = 't-run'",
        ),
        (
            "$.startedAt",
            "UPDATE orchestration_v2_projection_runs SET payload_json = json_remove(payload_json, '$.startedAt') WHERE run_id = 'r-run-2'",
        ),
        (
            "$.questions[0].question",
            "UPDATE orchestration_v2_projection_turn_items SET payload_json = '{\"questions\":[{}]}' WHERE turn_item_id = 'i-question'",
        ),
        (
            "$.failure.message",
            "UPDATE orchestration_v2_projection_turn_items SET payload_json = '{\"failure\":{}}' WHERE turn_item_id = 'i-err'",
        ),
        (
            "linkedAt",
            "UPDATE orchestration_v2_projection_threads SET payload_json = json_remove(payload_json, '$.pullRequests[0].linkedAt') WHERE thread_id = 't-prs'",
        ),
    ] {
        let f = T3Fixture::with_rows();
        f.exec(sql);
        let why = broken(&f);
        assert!(why.contains(key), "{key}: {why}");
    }
}

#[test]
fn optional_keys_may_be_missing() {
    let f = T3Fixture::with_rows();
    f.exec(
        "UPDATE orchestration_v2_projection_threads
         SET payload_json = json_remove(payload_json, '$.pullRequests', '$.lineage.sideChat')",
    );
    read(&f);
}

#[test]
fn a_missing_or_bad_environment_id_is_none() {
    let f = T3Fixture::with_rows();
    std::fs::write(f.dir().join("environment-id"), "not-a-uuid\n").unwrap();
    assert_eq!(open(&f).environment_id().unwrap(), None);
    std::fs::remove_file(f.dir().join("environment-id")).unwrap();
    assert_eq!(open(&f).environment_id().unwrap(), None);
}

// ---- The state rules (threads.rs) on the fixture ----

mod states {
    use std::collections::{HashMap, HashSet};

    use airboss_services::{RequestKind, ThreadState, ThreadView};
    use airboss_sync::fake::T3Fixture;
    use airboss_sync::threads;

    use super::{LINKED, read};

    fn views(f: &T3Fixture) -> (HashMap<String, ThreadView>, Vec<String>) {
        let ids: Vec<String> = LINKED.iter().map(|s| s.to_string()).collect();
        let out = threads::linked(&read(f), &ids, &HashMap::new());
        (
            out.views
                .into_iter()
                .map(|v| (v.key.r#ref.clone(), v))
                .collect(),
            out.itemless,
        )
    }

    #[test]
    fn each_state_row() {
        let (v, itemless) = views(&T3Fixture::with_rows());
        let state = |id: &str| v[id].state;
        assert_eq!(state("t-missing"), ThreadState::Gone, "no row");
        assert_eq!(state("t-archived"), ThreadState::Gone);
        assert_eq!(state("t-deleted"), ThreadState::Gone);
        assert_eq!(state("t-question"), ThreadState::NeedsMe, "before running");
        assert_eq!(state("t-approve"), ThreadState::NeedsMe);
        assert_eq!(state("t-run"), ThreadState::Running);
        assert_eq!(state("t-queued"), ThreadState::Running);
        assert_eq!(state("t-settled"), ThreadState::Settled);
        assert_eq!(state("t-errored"), ThreadState::Errored);
        assert_eq!(state("t-failed"), ThreadState::Errored);
        assert_eq!(state("t-idle"), ThreadState::Idle, "interrupted");
        assert_eq!(state("t-noruns"), ThreadState::Idle, "no runs");
        assert!(itemless.is_empty());
    }

    #[test]
    fn question_and_approval_text() {
        let (v, _) = views(&T3Fixture::with_rows());
        let q = v["t-question"].request.clone().unwrap();
        assert_eq!(
            (q.id.as_str(), q.kind, q.text.as_str(), q.at.as_str()),
            (
                "q-question",
                RequestKind::Question,
                "Ship it to staging now?",
                "2026-11-26T11:30:00.000Z"
            )
        );
        let a = v["t-approve"].request.clone().unwrap();
        assert_eq!(
            (a.kind, a.text.as_str()),
            (RequestKind::Approval, "cargo publish --dry-run")
        );
        let no_prompt = v["t-approve2"].request.clone().unwrap();
        assert_eq!(no_prompt.text, "Change files", "the kind's phrase");
    }

    #[test]
    fn a_childs_request_marks_its_root() {
        let (v, _) = views(&T3Fixture::with_rows());
        let root = &v["t-root"];
        assert_eq!(root.state, ThreadState::NeedsMe);
        assert_eq!(root.request.as_ref().unwrap().text, "Which schema?");
        assert_eq!(root.child_count, 2);
    }

    #[test]
    fn errors_and_details() {
        let (v, _) = views(&T3Fixture::with_rows());
        assert_eq!(
            v["t-errored"].error.as_deref(),
            Some("Usage limit reached. Try again at 18:00.")
        );
        assert_eq!(v["t-errored"].run_id.as_deref(), Some("r-errored-2"));
        assert_eq!(v["t-failed"].error.as_deref(), Some("Run failed"));
        let run = &v["t-run"];
        assert_eq!(run.branch.as_deref(), Some("feat/run"));
        assert_eq!(run.worktree_path.as_deref(), Some("/work/wt/run"));
        assert_eq!(run.project_path.as_deref(), Some("/work/web"));
        assert!(run.linked);
        assert_eq!(
            run.last_activity_at.as_deref(),
            Some("2026-11-26T11:00:00.000Z"),
            "the latest of requested and started"
        );
        assert_eq!(
            v["t-question"].last_activity_at.as_deref(),
            Some("2026-11-26T11:30:00.000Z"),
            "the request counts"
        );
        assert_eq!(
            v["t-noruns"].last_activity_at.as_deref(),
            Some("2026-11-26T04:00:00.000Z"),
            "no runs: the thread's own update"
        );
    }

    #[test]
    fn a_known_failure_keeps_its_message() {
        let f = T3Fixture::with_rows();
        let ids = vec!["t-errored".to_string()];
        let mut q = super::query();
        q.known_failures.insert("r-errored-2".into());
        let mut db = super::open(&f);
        use airboss_sync::t3::T3Source;
        let r = db.read(&q).unwrap();
        let known = HashMap::from([("r-errored-2".to_string(), "Earlier text".to_string())]);
        let out = threads::linked(&r, &ids, &known);
        assert_eq!(out.views[0].error.as_deref(), Some("Earlier text"));
    }

    #[test]
    fn a_request_without_its_item_is_retried() {
        let f = T3Fixture::with_rows();
        f.add_request(
            "t-run",
            "q-new",
            "command",
            "2026-11-26T11:50:00.000Z",
            None,
        );
        let (v, itemless) = views(&f);
        assert_eq!(v["t-run"].state, ThreadState::Running, "not needs_me yet");
        assert_eq!(itemless, vec!["q-new".to_string()]);
    }

    #[test]
    fn untracked_filtering() {
        let r = read(&T3Fixture::with_rows());
        let linked: HashSet<String> = ["t-run", "t-question"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let u = threads::untracked(r.untracked.as_ref().unwrap(), &linked);
        let ids: Vec<&str> = u.iter().map(|v| v.key.r#ref.as_str()).collect();
        assert!(!ids.contains(&"t-run") && !ids.contains(&"t-question"));
        assert!(ids.contains(&"t-idle"));
        assert!(u.iter().all(|v| !v.linked));
        let settled = u.iter().find(|v| v.key.r#ref == "t-settled").unwrap();
        assert_eq!(settled.state, ThreadState::Settled);
        let approve = u.iter().find(|v| v.key.r#ref == "t-approve").unwrap();
        assert_eq!(
            approve.state,
            ThreadState::Idle,
            "untracked runs aren't read"
        );
    }
}
