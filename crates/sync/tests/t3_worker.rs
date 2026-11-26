//! The T3 and Claude worker, driven with chosen times: the 2 s gap, `data_version`, the
//! 15-minute cap, a long sleep, broken and missing T3, notes, PRs found on threads,
//! following `core`, Claude sessions and the watches.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime};

use airboss_core::{LinkKind, Status, TaskId};
use airboss_services::{RequestKind, Source, ThreadKey, ThreadState, ThreadView};
use airboss_sync::fake::{
    FakeClaude, FakeHerdr, FakeT3, FakeT3Handle, FakeWatch, T3Fixture, TempDir,
};
use airboss_sync::t3::T3Error;
use airboss_sync::{Env, GhMsg, Hub, Step, T3Msg, T3Worker, WatchTarget};
use common::{Core, at, cfg, ms, t0};

struct Rig {
    core: Core,
    t3: T3Fixture,
    claude_dir: TempDir,
    claude: FakeClaude,
    fake_t3: FakeT3Handle,
    herdr: FakeHerdr,
    watch: FakeWatch,
    hub: Arc<Hub>,
    gh: Receiver<GhMsg>,
    worker: T3Worker,
}

impl Rig {
    /// A worker over the fixture, with `links` (task title, thread) made first.
    fn new(threads: &[&str]) -> (Rig, Vec<TaskId>) {
        Rig::with(T3Fixture::with_rows(), threads)
    }

    fn with(t3: T3Fixture, threads: &[&str]) -> (Rig, Vec<TaskId>) {
        let core = Core::new();
        let tasks: Vec<TaskId> = threads
            .iter()
            .map(|t| {
                let id = core.task(&format!("task for {t}"));
                core.link(id, LinkKind::T3Thread, t);
                id
            })
            .collect();
        let claude_dir = TempDir::new();
        let claude = FakeClaude::new(claude_dir.path());
        let fake = FakeT3::new(t3.dir());
        let fake_t3 = fake.handle();
        let watch = FakeWatch::new();
        let herdr = FakeHerdr::new();
        let hub = Hub::new();
        let (me, _rx) = channel();
        std::mem::forget(_rx);
        let (gh_tx, gh) = channel();
        let env = Env::new(
            cfg(t3.dir(), claude_dir.path()),
            core.store.clone(),
            hub.clone(),
        );
        let worker = T3Worker::new(
            env,
            Box::new(fake),
            Box::new(claude.clone()),
            Box::new(herdr.clone()),
            Box::new(watch.clone()),
            me,
            gh_tx,
            core.open(),
        )
        .unwrap();
        let mut rig = Rig {
            core,
            t3,
            claude_dir,
            claude,
            fake_t3,
            herdr,
            watch,
            hub,
            gh,
            worker,
        };
        rig.core.mark();
        (rig, tasks)
    }

    fn start(&mut self, now: SystemTime) -> Step {
        let step = self.worker.start(now);
        assert!(step.next_wake > now);
        step
    }

    fn send(&mut self, msg: T3Msg, now: SystemTime) -> Step {
        let step = self.worker.handle(msg, now);
        assert!(step.next_wake > now, "next_wake must be after now");
        assert!(step.next_wake <= now + Duration::from_secs(15 * 60));
        step
    }

    fn view(&self, r: &str) -> Option<ThreadView> {
        self.hub
            .snapshot()
            .threads
            .get(&ThreadKey {
                source: Source::T3,
                r#ref: r.into(),
            })
            .cloned()
    }

    fn state(&self, r: &str) -> Option<ThreadState> {
        self.view(r).map(|v| v.state)
    }

    fn follow(&mut self, now: SystemTime) -> Step {
        let s = self.core.summary();
        self.send(T3Msg::Store(s), now)
    }

    fn t3_connection(&self) -> airboss_services::Connection {
        self.hub
            .connections()
            .into_iter()
            .find(|c| c.name == "T3 Code")
            .unwrap()
    }
}

#[test]
fn the_first_pass_reads_linked_threads_untracked_threads_and_projects() {
    let (mut rig, tasks) = Rig::new(&["t-question", "t-run", "t-missing"]);
    let step = rig.start(t0());
    assert!(step.changed);
    let q = rig.view("t-question").unwrap();
    assert_eq!(q.state, ThreadState::NeedsMe);
    let r = q.request.unwrap();
    assert_eq!(
        (r.kind, r.text.as_str()),
        (RequestKind::Question, "Ship it to staging now?")
    );
    assert_eq!(rig.state("t-run"), Some(ThreadState::Running));
    assert_eq!(rig.state("t-missing"), Some(ThreadState::Gone));
    let snap = rig.hub.snapshot();
    let key = |r: &str| ThreadKey {
        source: Source::T3,
        r#ref: r.into(),
    };
    assert_eq!(
        snap.thread_tasks.get(&key("t-question")),
        Some(&vec![tasks[0]])
    );
    assert_eq!(snap.threads_for(tasks[1]).len(), 1);
    let untracked: Vec<&str> = snap
        .untracked()
        .iter()
        .map(|v| v.key.r#ref.as_str())
        .collect();
    assert!(untracked.contains(&"t-idle"));
    assert!(
        !untracked.contains(&"t-question"),
        "linked threads aren't untracked"
    );
    let names: Vec<&str> = snap.projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["web", "ui", "api"]);
    assert!(snap.checked_at.is_some());
    assert!(rig.t3_connection().ok);
    assert_eq!(rig.t3_connection().state, "Connected");
    assert!(rig.hub.environment().is_some());
}

#[test]
fn passes_keep_a_two_second_gap() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    let reads = rig.fake_t3.reads();
    rig.t3.add_request(
        "t-run",
        "q-new",
        "user_input",
        "2026-11-26T12:00:00.500Z",
        Some(r#"{"questions":[{"question":"Now?"}]}"#),
    );
    let step = rig.send(T3Msg::Wal, t0() + ms(500));
    assert_eq!(step.next_wake, at(2), "a pass waits for the end of the gap");
    assert_eq!(rig.fake_t3.reads(), reads);
    assert_eq!(rig.state("t-run"), Some(ThreadState::Running));
    // More events inside the gap still mean one pass at its end.
    let step = rig.send(T3Msg::Wal, t0() + ms(1500));
    assert_eq!(step.next_wake, at(2));
    let step = rig.send(T3Msg::Tick, at(2));
    assert!(step.changed);
    assert_eq!(rig.fake_t3.reads(), reads + 1);
    assert_eq!(rig.state("t-run"), Some(ThreadState::NeedsMe));
    // The question reached the snapshot within 3 s of T3 writing it.
}

#[test]
fn an_unchanged_data_version_skips_the_read() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    let reads = rig.fake_t3.reads();
    let step = rig.send(T3Msg::Wal, at(5));
    assert!(!step.changed);
    assert_eq!(
        rig.fake_t3.reads(),
        reads,
        "nothing committed, nothing read"
    );
    rig.t3.exec("UPDATE orchestration_v2_projection_threads SET title = 'Renamed' WHERE thread_id = 't-run'");
    rig.send(T3Msg::Wal, at(10));
    assert_eq!(rig.fake_t3.reads(), reads + 1);
    assert_eq!(rig.view("t-run").unwrap().title.as_deref(), Some("Renamed"));
}

#[test]
fn an_idle_worker_waits_at_most_fifteen_minutes() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    let step = rig.start(t0());
    assert_eq!(step.next_wake, at(15 * 60));
    let step = rig.send(T3Msg::Tick, at(15 * 60));
    assert_eq!(step.next_wake, at(30 * 60));
}

#[test]
fn after_a_six_hour_sleep_woke_reads_what_changed() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    rig.t3.exec("UPDATE orchestration_v2_projection_runs SET status = 'completed', completed_at = '2026-11-26T13:00:00.000Z' WHERE run_id = 'r-run-2'");
    let step = rig.send(T3Msg::Woke, at(6 * 3600));
    assert!(step.changed);
    assert_eq!(rig.state("t-run"), Some(ThreadState::Idle));
    assert_eq!(step.next_wake, at(6 * 3600 + 15 * 60));
}

#[test]
fn rows_are_written_only_when_something_changed() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    let db = rusqlite::Connection::open(rig.core.dir.path().join("airboss.db")).unwrap();
    let changed_at = |db: &rusqlite::Connection| -> String {
        db.query_row(
            "SELECT changed_at FROM observed_threads WHERE ref = 't-run'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let first = changed_at(&db);
    rig.send(T3Msg::Refresh, at(60));
    assert_eq!(changed_at(&db), first, "the same view isn't written again");
    rig.t3.exec("UPDATE orchestration_v2_projection_threads SET title = 'Renamed' WHERE thread_id = 't-run'");
    rig.send(T3Msg::Wal, at(120));
    assert_ne!(changed_at(&db), first);
}

#[test]
fn an_unknown_value_breaks_t3_sync_and_recovery_clears_it() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    let changes = rig.hub.changes().current();
    rig.t3.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'paused' WHERE run_id = 'r-run-2'",
    );
    rig.send(T3Msg::Wal, at(5));
    let c = rig.t3_connection();
    assert!(!c.ok);
    assert_eq!(c.state, "Not updating since 12:00 UTC");
    assert!(c.fix.unwrap().contains("paused"));
    let v = rig.view("t-run").unwrap();
    assert!(v.stale, "the last known state stays, marked stale");
    assert_eq!(v.state, ThreadState::Running);
    assert!(
        rig.hub.changes().current() > changes,
        "the connection changed"
    );

    rig.t3.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'running' WHERE run_id = 'r-run-2'",
    );
    let before = rig.hub.changes().current();
    let step = rig.send(T3Msg::Wal, at(10));
    assert!(step.changed);
    assert!(rig.t3_connection().ok);
    assert!(!rig.view("t-run").unwrap().stale);
    assert!(
        rig.hub.changes().current() > before,
        "recovery bumps changes"
    );
}

#[test]
fn busy_is_skipped_and_retried() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    rig.fake_t3.fail_next(T3Error::Busy);
    rig.send(T3Msg::Wal, at(5));
    assert!(rig.t3_connection().ok, "busy isn't broken");
    rig.t3.exec(
        "UPDATE orchestration_v2_projection_threads SET title = 'Later' WHERE thread_id = 't-run'",
    );
    rig.send(T3Msg::Wal, at(10));
    assert_eq!(rig.view("t-run").unwrap().title.as_deref(), Some("Later"));
}

#[test]
fn missing_t3_is_off_until_it_appears() {
    let dir = TempDir::new();
    // A T3 folder with no database: build the rig on an empty fixture, then move the file.
    let (mut rig, _) = Rig::with(T3Fixture::empty(), &[]);
    let db = rig.t3.dir().join("statev2.sqlite");
    let saved = dir.path().join("saved.sqlite");
    std::fs::rename(&db, &saved).unwrap();
    rig.start(t0());
    let c = rig.t3_connection();
    assert_eq!((c.ok, c.state.as_str()), (false, "Not installed"));
    std::fs::rename(&saved, &db).unwrap();
    rig.send(T3Msg::Tick, at(300));
    assert!(rig.t3_connection().ok, "checked again on the GitHub tick");
}

#[test]
fn a_failed_run_writes_one_note_per_failed_run() {
    let (mut rig, tasks) = Rig::new(&["t-errored"]);
    rig.start(t0());
    let note = "Run failed: Usage limit reached. Try again at 18:00.".to_string();
    assert_eq!(
        rig.core.notes(tasks[0]),
        vec![("sync:t3".to_string(), note.clone())]
    );
    rig.send(T3Msg::Refresh, at(60));
    assert_eq!(
        rig.core.notes(tasks[0]).len(),
        1,
        "the same run isn't noted twice"
    );
    rig.t3
        .add_run("t-errored", 3, "failed", "2026-11-26T12:01:00.000Z");
    rig.send(T3Msg::Wal, at(70));
    let notes = rig.core.notes(tasks[0]);
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[1].1, "Run failed", "no error item");
}

#[test]
fn a_failure_message_written_after_the_run_failed_is_still_read() {
    let (mut rig, _) = Rig::new(&["t-failed"]);
    rig.start(t0());
    assert_eq!(
        rig.view("t-failed").unwrap().error.as_deref(),
        Some("Run failed")
    );
    rig.t3.exec(
        r#"INSERT INTO orchestration_v2_projection_turn_items (turn_item_id, thread_id, run_id, node_id, ordinal, type, status, updated_at, payload_json)
           VALUES ('i-late', 't-failed', 'r-failed-1', 'n-late', 1, 'error', 'failed', '2026-11-26T12:00:01.000Z', '{"failure":{"message":"Out of disk"}}')"#,
    );
    rig.send(T3Msg::Wal, at(5));
    assert_eq!(
        rig.view("t-failed").unwrap().error.as_deref(),
        Some("Out of disk"),
        "the fallback wasn't cached"
    );
}

#[test]
fn linked_claude_sessions_are_gone_when_the_folder_goes() {
    let (mut rig, _) = Rig::new(&[]);
    let task = rig.core.task("Claude work");
    rig.core.link(task, LinkKind::ClaudeSession, "s-1");
    rig.follow(t0());
    rig.claude.write(10, "s-1", "busy", "cli");
    rig.start(t0());
    std::fs::remove_dir_all(rig.claude_dir.path()).unwrap();
    rig.send(T3Msg::ClaudeDir, at(1));
    let key = ThreadKey {
        source: Source::Claude,
        r#ref: "s-1".into(),
    };
    assert_eq!(rig.hub.snapshot().threads[&key].state, ThreadState::Gone);
}

#[test]
fn the_note_survives_a_restart_without_repeating() {
    let (mut rig, tasks) = Rig::new(&["t-errored"]);
    rig.start(t0());
    let env = Env::new(
        cfg(rig.t3.dir(), rig.claude_dir.path()),
        rig.core.store.clone(),
        Hub::new(),
    );
    let (me, _r1) = channel();
    let (gh, _r2) = channel();
    let mut again = T3Worker::new(
        env,
        Box::new(FakeT3::new(rig.t3.dir())),
        Box::new(rig.claude.clone()),
        Box::new(FakeHerdr::new()),
        Box::new(FakeWatch::new()),
        me,
        gh,
        rig.core.open(),
    )
    .unwrap();
    again.start(at(100));
    assert_eq!(rig.core.notes(tasks[0]).len(), 1);
}

#[test]
fn only_other_states_never_write_notes() {
    let (mut rig, tasks) = Rig::new(&["t-question", "t-run", "t-settled"]);
    rig.start(t0());
    for t in tasks {
        assert!(
            rig.core.notes(t).is_empty(),
            "questions and approvals aren't history"
        );
    }
}

#[test]
fn prs_on_a_thread_count_only_if_linked_after_our_thread_link() {
    let (mut rig, tasks) = Rig::new(&["t-prs"]);
    let linked = rig.core.linked_at(tasks[0], "t-prs");
    let ts = |t: SystemTime| airboss_services::time::to_ts(t);
    let after = ts(linked + Duration::from_secs(60));
    let before = ts(linked - Duration::from_secs(86_400));
    rig.t3.exec(&format!(
        "UPDATE orchestration_v2_projection_threads SET payload_json =
           json_set(payload_json, '$.pullRequests[0].linkedAt', '{after}',
                                  '$.pullRequests[1].linkedAt', '{before}',
                                  '$.pullRequests[2].linkedAt', '{after}')
         WHERE thread_id = 't-prs'"
    ));
    rig.start(t0());
    let reads = |rig: &Rig| -> Vec<Vec<String>> {
        rig.gh
            .try_iter()
            .filter_map(|m| match m {
                GhMsg::Read(prs) => Some(prs.into_iter().map(|p| p.to_string()).collect()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        reads(&rig),
        vec![vec!["acme/web#7".to_string()]],
        "github.com only, at or after the link, canonical"
    );
    rig.send(T3Msg::Refresh, at(60));
    assert!(reads(&rig).is_empty(), "already waiting: not asked again");
}

#[test]
fn a_request_without_its_item_is_retried_then_broken() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    rig.t3.add_request(
        "t-run",
        "q-lag",
        "command",
        "2026-11-26T12:00:05.000Z",
        None,
    );
    let step = rig.send(T3Msg::Wal, at(5));
    assert_eq!(rig.state("t-run"), Some(ThreadState::Running));
    assert_eq!(step.next_wake, at(15), "checked again when the grace ends");
    rig.send(T3Msg::Tick, at(15));
    let c = rig.t3_connection();
    assert!(!c.ok);
    assert!(c.fix.unwrap().contains("q-lag"));
}

#[test]
fn a_late_item_arrives_within_the_grace() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    rig.t3.add_request(
        "t-run",
        "q-lag",
        "command",
        "2026-11-26T12:00:05.000Z",
        None,
    );
    rig.send(T3Msg::Wal, at(5));
    rig.t3.add_item(
        "t-run",
        "q-lag",
        "approval_request",
        "2026-11-26T12:00:06.000Z",
        r#"{"prompt":"ls"}"#,
    );
    rig.send(T3Msg::Wal, at(8));
    assert_eq!(rig.state("t-run"), Some(ThreadState::NeedsMe));
    assert!(rig.t3_connection().ok);
}

#[test]
fn a_new_thread_link_is_read_at_once_and_unlinking_drops_it() {
    let (mut rig, _) = Rig::new(&["t-run"]);
    rig.start(t0());
    let task = rig.core.task("Another");
    rig.core.link(task, LinkKind::T3Thread, "t-settled");
    let step = rig.follow(t0() + ms(100));
    assert!(step.changed, "inside the gap, but a new link skips it");
    assert_eq!(rig.state("t-settled"), Some(ThreadState::Settled));
    assert_eq!(rig.hub.snapshot().threads_for(task).len(), 1);

    rig.core.set_status(task, Status::Done);
    rig.follow(at(10));
    assert!(
        rig.hub.snapshot().threads_for(task).is_empty(),
        "done tasks drop out"
    );
    assert_eq!(
        rig.state("t-settled"),
        None,
        "linked only to a done task: not shown, not untracked"
    );
}

#[test]
fn find_thread_answers_from_q7() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    let (tx, rx) = channel();
    rig.send(
        T3Msg::FindThread {
            project: "/work/web/src".into(),
            branch: "feat/run".into(),
            reply: tx,
        },
        at(1),
    );
    assert_eq!(rx.recv().unwrap().as_deref(), Some("t-web-new"));
}

#[test]
fn watches_cover_t3_and_claude_and_are_re_armed() {
    let (mut rig, _) = Rig::new(&[]);
    rig.claude.write(3080, "s-1", "busy", "cli");
    rig.start(t0());
    let last = rig.watch.calls().last().cloned().unwrap();
    let t3 = rig.t3.dir().to_path_buf();
    assert!(last.contains(&WatchTarget::T3Dir(t3.clone())));
    assert!(last.contains(&WatchTarget::T3Wal(t3.join("statev2.sqlite-wal"))));
    assert!(last.contains(&WatchTarget::ClaudeDir(rig.claude_dir.path().to_path_buf())));
    assert!(last.contains(&WatchTarget::ClaudeFile(
        rig.claude_dir.path().join("3080.json")
    )));
    let n = rig.watch.calls().len();
    rig.send(T3Msg::T3Dir, at(5));
    assert_eq!(
        rig.watch.calls().len(),
        n + 1,
        "a folder event re-arms the WAL watch"
    );
}

#[test]
fn a_new_claude_session_file_without_its_status_is_watched_not_broken() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    let file = rig.claude_dir.path().join("67020.json");
    std::fs::write(
        &file,
        r#"{"pid": 67020, "sessionId": "s-new", "procStart": "p"}"#,
    )
    .unwrap();
    rig.send(T3Msg::ClaudeDir, at(1));
    let claude_row = |rig: &Rig| {
        rig.hub
            .connections()
            .iter()
            .any(|c| c.name == "Claude Code")
    };
    assert!(
        !claude_row(&rig),
        "a file caught mid-write doesn't break Claude sync"
    );
    let last = rig.watch.calls().last().cloned().unwrap();
    assert!(last.contains(&WatchTarget::ClaudeFile(file.clone())));

    // Claude Code finishes the file in place; its watch reads it.
    rig.claude.write(67020, "s-new", "waiting", "cli");
    rig.send(T3Msg::ClaudeFile(file), at(2));
    assert!(!claude_row(&rig));
    let key = ThreadKey {
        source: Source::Claude,
        r#ref: "s-new".into(),
    };
    assert_eq!(rig.hub.snapshot().threads[&key].state, ThreadState::NeedsMe);
}

#[test]
fn a_watch_failure_says_so() {
    let (mut rig, _) = Rig::new(&[]);
    rig.watch.fail_with("too many open files");
    rig.start(t0());
    let c = rig.t3_connection();
    assert_eq!((c.ok, c.state.as_str()), (false, "Not live"));
}

#[test]
fn claude_sessions_come_and_go() {
    let (mut rig, _) = Rig::new(&[]);
    let task = rig.core.task("Claude work");
    rig.core.link(task, LinkKind::ClaudeSession, "s-linked");
    rig.follow(t0());
    rig.start(t0());
    rig.claude.write(10, "s-linked", "busy", "cli");
    rig.claude.write(11, "s-free", "idle", "cli");
    rig.send(T3Msg::ClaudeDir, at(1));
    let key = |r: &str| ThreadKey {
        source: Source::Claude,
        r#ref: r.into(),
    };
    let snap = rig.hub.snapshot();
    assert_eq!(snap.threads[&key("s-linked")].state, ThreadState::Running);
    assert!(snap.threads[&key("s-linked")].linked);
    assert!(!snap.threads[&key("s-free")].linked);
    assert_eq!(snap.threads_for(task).len(), 1);

    // The linked session's process ends; the GitHub tick's liveness check finds it.
    rig.claude.kill(10);
    rig.claude.kill(11);
    let calls = rig.claude.alive_calls();
    rig.send(T3Msg::Tick, at(300));
    assert_eq!(rig.claude.alive_calls(), calls + 1);
    let snap = rig.hub.snapshot();
    assert_eq!(snap.threads[&key("s-linked")].state, ThreadState::Gone);
    assert!(
        !snap.threads.contains_key(&key("s-free")),
        "a dead untracked session goes"
    );
}

#[test]
fn liveness_runs_only_when_the_files_change_or_on_ticks() {
    let (mut rig, _) = Rig::new(&[]);
    rig.claude.write(10, "s", "busy", "cli");
    rig.start(t0());
    let calls = rig.claude.alive_calls();
    rig.claude.write(10, "s", "idle", "cli");
    rig.send(
        T3Msg::ClaudeFile(rig.claude_dir.path().join("10.json")),
        at(1),
    );
    assert_eq!(rig.claude.alive_calls(), calls, "same files: no ps");
    rig.claude.write(12, "t", "idle", "cli");
    rig.send(T3Msg::ClaudeDir, at(2));
    assert_eq!(rig.claude.alive_calls(), calls + 1, "a new file: ps");
}

#[test]
fn an_unknown_claude_status_leaves_claude_sync_working() {
    let (mut rig, _) = Rig::new(&[]);
    rig.claude.write(10, "s", "dreaming", "cli");
    rig.claude.write(11, "bg", "shell", "cli");
    rig.start(t0());
    let rows = rig.hub.connections();
    assert!(
        rows.iter().all(|c| c.name != "Claude Code"),
        "a status Claude Code added later doesn't break Claude sync: {rows:?}"
    );
    // "Agents" is the app's row for the airboss command; sharing the name would merge them.
    assert!(rows.iter().all(|c| c.name != "Agents"));
    let state = |r: &str| {
        rig.hub.snapshot().threads[&ThreadKey {
            source: Source::Claude,
            r#ref: r.into(),
        }]
            .state
    };
    assert_eq!(state("s"), ThreadState::Idle);
    assert_eq!(state("bg"), ThreadState::Running);
}

#[test]
fn no_claude_folder_means_no_claude_code_row() {
    let (mut rig, _) = Rig::new(&[]);
    std::fs::remove_dir_all(rig.claude_dir.path()).unwrap();
    rig.start(t0());
    assert!(
        rig.hub
            .connections()
            .iter()
            .all(|c| c.name != "Claude Code" && c.name != "Agents")
    );
}

// ---- Herdr ----

/// A snapshot line as `hc events --json` writes it: `(address, state, status word)`.
fn herdr_snapshot(seq: u64, terminals: &[(&str, &str, Option<&str>)]) -> String {
    let list: Vec<serde_json::Value> = terminals
        .iter()
        .map(|(address, state, status)| {
            serde_json::json!({
                "address": address, "argv": ["claude"], "cwd": format!("/work/{address}"),
                "labels": {}, "state": state, "pid": 4242, "exit_code": null,
                "status": status.map(|s| serde_json::json!({"app": "claude-code", "state": s})),
                "status_ms": status.map(|_| 1_795_600_000_000_u64),
                "title": format!("title of {address}"), "created_ms": 1_795_500_000_000_u64,
                "finished_ms": null, "screen": "exact", "turn": 0,
            })
        })
        .collect();
    serde_json::json!({"channel": null, "seq": seq, "terminals": list}).to_string()
}

/// An `event` notification line.
fn herdr_event(seq: u64, address: &str, kind: &str, data: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "method": "event",
        "params": {"data": data, "seq": seq, "terminal": address, "type": kind},
    })
    .to_string()
}

fn status_event(seq: u64, address: &str, word: Option<&str>) -> String {
    herdr_event(
        seq,
        address,
        "status",
        serde_json::json!({"records": {}, "reported": true,
                           "status": word.map(|w| serde_json::json!({"state": w}))}),
    )
}

fn herdr_key(address: &str) -> ThreadKey {
    ThreadKey {
        source: Source::Herdr,
        r#ref: address.into(),
    }
}

impl Rig {
    /// Links each address to a task of its own, and follows.
    fn link_terminals(&mut self, addresses: &[&str], now: SystemTime) -> Vec<TaskId> {
        let tasks = addresses
            .iter()
            .map(|a| {
                let t = self.core.task(&format!("task for {a}"));
                self.core.link(t, LinkKind::HerdrTerminal, a);
                t
            })
            .collect();
        self.follow(now);
        tasks
    }

    /// Hands the worker one line of the running stream.
    fn herdr_line(&mut self, line: &str, now: SystemTime) -> Step {
        match self.herdr.line(line) {
            Some(msg) => self.send(msg, now),
            None => self.send(T3Msg::Tick, now),
        }
    }

    fn herdr_view(&self, address: &str) -> Option<ThreadView> {
        self.hub
            .snapshot()
            .threads
            .get(&herdr_key(address))
            .cloned()
    }

    fn herdr_state(&self, address: &str) -> Option<ThreadState> {
        self.herdr_view(address).map(|v| v.state)
    }

    fn herdr_connection(&self) -> Option<airboss_services::Connection> {
        self.hub
            .connections()
            .into_iter()
            .find(|c| c.name == "Herdr")
    }
}

#[test]
fn each_herdr_state_maps_as_decision_6_says() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    assert!(rig.herdr.running(), "hc events starts with the worker");
    let all = ["t1", "t2", "t3x", "t4", "t5", "t6", "t7", "t8"];
    rig.link_terminals(&all, at(1));
    rig.herdr_line(
        &herdr_snapshot(
            10,
            &[
                ("t1", "live", Some("working")),
                ("t2", "live", Some("blocked")),
                ("t3x", "live", Some("error")),
                ("t4", "live", Some("done")),
                ("t5", "live", Some("idle")),
                ("t6", "live", None),
                ("t7", "finished", Some("working")),
                // t8 isn't listed.
            ],
        ),
        at(2),
    );
    let states: Vec<Option<ThreadState>> = all.iter().map(|a| rig.herdr_state(a)).collect();
    assert_eq!(
        states,
        vec![
            Some(ThreadState::Running),
            Some(ThreadState::NeedsMe),
            Some(ThreadState::Errored),
            Some(ThreadState::Idle),
            Some(ThreadState::Idle),
            Some(ThreadState::Idle),
            Some(ThreadState::Gone),
            Some(ThreadState::Gone),
        ]
    );
    let v = rig.herdr_view("t2").unwrap();
    let request = v.request.clone().unwrap();
    assert_eq!(
        (request.id.as_str(), request.kind, request.text.as_str()),
        (
            "herdr/t2/1795600000000",
            RequestKind::Question,
            "Waiting on you in Herdr."
        ),
        "no kind or message: a plain question"
    );
    assert_eq!(v.title.as_deref(), Some("title of t2"));
    assert_eq!(v.project_path.as_deref(), Some("/work/t2"));
    assert_eq!(
        v.last_activity_at.as_deref(),
        Some("2026-11-25T09:46:40.000Z"),
        "status_ms"
    );
    assert!(v.linked && !v.stale);
    let snap = rig.hub.snapshot();
    let tasks = &snap.thread_tasks[&herdr_key("t1")];
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        rig.herdr_connection().map(|c| (c.ok, c.state)),
        Some((true, "Connected".to_string()))
    );
}

#[test]
fn herdr_terminals_nobody_linked_are_not_reported() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(1));
    rig.herdr_line(
        &herdr_snapshot(
            1,
            &[
                ("t1", "live", Some("working")),
                ("t9", "live", Some("blocked")),
            ],
        ),
        at(2),
    );
    let snap = rig.hub.snapshot();
    let herdr: Vec<&ThreadKey> = snap
        .threads
        .keys()
        .filter(|k| k.source == Source::Herdr)
        .collect();
    assert_eq!(herdr, vec![&herdr_key("t1")]);
}

#[test]
fn herdr_change_lines_follow_the_snapshot() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1", "vps/t7"], at(1));
    rig.herdr_line(&herdr_snapshot(100, &[("t1", "live", None)]), at(2));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Idle));
    assert_eq!(rig.herdr_state("vps/t7"), Some(ThreadState::Gone));

    let step = rig.herdr_line(&status_event(101, "t1", Some("working")), at(3));
    assert!(step.changed);
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Running));
    assert_eq!(
        rig.herdr_view("t1").unwrap().last_activity_at.as_deref(),
        Some("2026-11-26T12:00:03.000Z"),
        "a status event carries no status_ms: it's when it arrived"
    );
    rig.herdr_line(
        &herdr_event(
            102,
            "t1",
            "title",
            serde_json::json!({"title": "Fix the bug"}),
        ),
        at(4),
    );
    assert_eq!(
        rig.herdr_view("t1").unwrap().title.as_deref(),
        Some("Fix the bug")
    );
    rig.herdr_line(&status_event(103, "t1", Some("blocked")), at(5));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::NeedsMe));
    rig.herdr_line(&status_event(104, "t1", Some("done")), at(6));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Idle));

    // A terminal on another machine, created after the snapshot.
    let created = herdr_event(
        105,
        "vps/t7",
        "created",
        serde_json::json!({"address": "vps/t7", "state": "live", "status": null,
                           "status_ms": null, "title": "", "name": "deploy", "cwd": "/srv"}),
    );
    rig.herdr_line(&created, at(7));
    let v = rig.herdr_view("vps/t7").unwrap();
    assert_eq!(v.state, ThreadState::Idle);
    assert_eq!(v.title.as_deref(), Some("deploy"), "no title: its name");
    rig.herdr_line(&status_event(106, "vps/t7", Some("error")), at(8));
    assert_eq!(rig.herdr_state("vps/t7"), Some(ThreadState::Errored));

    // Lines that change nothing sync follows never reach the worker.
    let heartbeat = r#"{"jsonrpc":"2.0","method":"heartbeat","params":{"seq":106}}"#;
    assert!(rig.herdr.line(heartbeat).is_none());
    let size = herdr_event(
        107,
        "t1",
        "size",
        serde_json::json!({"cols": 80, "rows": 24}),
    );
    assert!(rig.herdr.line(&size).is_none());

    rig.herdr_line(
        &herdr_event(
            108,
            "t1",
            "finished",
            serde_json::json!({"exit_code": 0, "finished_ms": 1, "reason": "exited"}),
        ),
        at(9),
    );
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Gone));
    rig.herdr_line(
        &herdr_event(109, "vps/t7", "removed", serde_json::json!({})),
        at(10),
    );
    assert_eq!(rig.herdr_state("vps/t7"), Some(ThreadState::Gone));
}

#[test]
fn herdr_events_already_in_a_snapshot_are_ignored() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(1));
    rig.herdr_line(
        &herdr_snapshot(50, &[("t1", "live", Some("working"))]),
        at(2),
    );
    // A reader that fell behind: `missed`, a fresh snapshot, then a late event from before it.
    let missed = r#"{"jsonrpc":"2.0","method":"missed","params":{"seq":60}}"#;
    assert!(rig.herdr.line(missed).is_none());
    rig.herdr_line(&herdr_snapshot(60, &[("t1", "live", Some("done"))]), at(3));
    rig.herdr_line(&status_event(55, "t1", Some("working")), at(4));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Idle));
    rig.herdr_line(&status_event(61, "t1", Some("working")), at(5));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Running));
}

#[test]
fn a_herdr_snapshot_without_a_terminal_drops_it() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(1));
    rig.herdr_line(
        &herdr_snapshot(1, &[("t1", "live", Some("working"))]),
        at(2),
    );
    // A restarted stream's snapshot no longer lists it: it was removed meanwhile.
    rig.send(T3Msg::Woke, at(3));
    rig.herdr_line(&herdr_snapshot(9, &[]), at(4));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Gone));
}

#[test]
fn a_herdr_link_made_later_shows_at_once_and_an_unlink_drops_it() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.herdr_line(
        &herdr_snapshot(1, &[("t4p", "live", Some("working"))]),
        at(1),
    );
    assert_eq!(rig.herdr_state("t4p"), None);
    let tasks = rig.link_terminals(&["t4p"], at(2));
    assert_eq!(rig.herdr_state("t4p"), Some(ThreadState::Running));
    let link = rig.core.lock().links(tasks[0]).unwrap()[0].id;
    rig.core
        .lock()
        .unlink(link, &airboss_core::Actor::Me)
        .unwrap();
    rig.follow(at(3));
    assert_eq!(rig.herdr_state("t4p"), None);
}

#[test]
fn hc_events_restarts_with_backoff_and_is_broken_after_three_failed_restarts() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(0));
    rig.herdr_line(
        &herdr_snapshot(1, &[("t1", "live", Some("working"))]),
        at(1),
    );
    assert_eq!(rig.herdr.follows(), 1);

    // It ends: restarted 1 s later, then 2, then 4 s after each failed restart.
    let mut now = at(2);
    for (n, secs) in [1, 2, 4, 8].into_iter().enumerate() {
        let wait = Duration::from_secs(secs);
        let step = rig.send(rig.herdr.ended("hc events stopped"), now);
        assert!(!rig.herdr.running(), "the child is reaped at once");
        assert!(step.next_wake <= now + wait, "woken for the restart");
        // Nothing happens before the backoff ends.
        rig.send(T3Msg::Tick, now + wait - ms(1));
        assert!(!rig.herdr.running());
        now += wait;
        rig.send(T3Msg::Tick, now);
        assert!(rig.herdr.running(), "restarted after {wait:?}");
        if n < 3 {
            assert_eq!(
                rig.herdr_connection().map(|c| c.ok),
                Some(true),
                "a restart that hasn't failed three times is still fine"
            );
            assert!(!rig.herdr_view("t1").unwrap().stale);
        }
    }
    let row = rig.herdr_connection().unwrap();
    assert!(!row.ok);
    assert_eq!(row.state, "Herdr's status can't be read");
    assert!(row.fix.unwrap().contains("hc events stopped"));
    assert!(
        rig.herdr_view("t1").unwrap().stale,
        "last known state, marked stale"
    );

    // The waits keep doubling to a minute.
    for secs in [16, 32, 60, 60] {
        rig.send(rig.herdr.ended("hc events stopped"), now);
        rig.send(T3Msg::Tick, now + Duration::from_secs(secs) - ms(1));
        assert!(!rig.herdr.running());
        now += Duration::from_secs(secs);
        rig.send(T3Msg::Tick, now);
        assert!(rig.herdr.running(), "restarted after {secs} s");
    }

    // A snapshot from the restarted stream: live again.
    rig.herdr_line(&herdr_snapshot(2, &[("t1", "live", Some("done"))]), now);
    let row = rig.herdr_connection().unwrap();
    assert_eq!((row.ok, row.state.as_str()), (true, "Connected"));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Idle));
    assert!(!rig.herdr_view("t1").unwrap().stale);

    // After a steady minute, an end starts a new streak: 1 s again.
    now += Duration::from_secs(61);
    rig.send(rig.herdr.ended("hc events stopped"), now);
    rig.send(T3Msg::Tick, now + Duration::from_secs(1));
    assert!(rig.herdr.running());
}

#[test]
fn hc_failing_to_start_counts_as_a_failure() {
    let (mut rig, _) = Rig::new(&[]);
    rig.herdr
        .fail_next(std::io::Error::other("permission denied"));
    let step = rig.start(t0());
    assert!(!rig.herdr.running());
    assert_eq!(step.next_wake, at(1));
    rig.send(T3Msg::Tick, at(1));
    assert!(rig.herdr.running());
}

#[test]
fn a_line_from_a_replaced_stream_is_ignored() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(0));
    rig.herdr_line(
        &herdr_snapshot(1, &[("t1", "live", Some("working"))]),
        at(1),
    );
    let old = rig
        .herdr
        .line(&status_event(2, "t1", Some("blocked")))
        .unwrap();
    let old_end = rig.herdr.ended("killed");
    let before = rig.herdr.stream();
    rig.send(T3Msg::Woke, at(2));
    assert_ne!(rig.herdr.stream(), before, "woke reads Herdr afresh");
    assert_eq!(rig.herdr.follows(), 2);
    rig.send(old, at(3));
    rig.send(old_end, at(3));
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Running));
    assert!(
        rig.herdr.running(),
        "the old stream's end doesn't stop the new one"
    );
}

#[test]
fn without_hc_herdr_is_off_with_no_row_and_linked_terminals_are_gone() {
    let (mut rig, _) = Rig::new(&[]);
    rig.herdr.set_missing(true);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(1));
    assert_eq!(rig.herdr_connection(), None);
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Gone));
    let step = rig.send(T3Msg::Tick, at(2));
    assert!(
        step.next_wake >= at(2) + Duration::from_secs(15 * 60),
        "no retry timer while it's missing"
    );
    // Installed meanwhile: the next GitHub tick finds it.
    rig.herdr.set_missing(false);
    rig.send(T3Msg::Tick, at(300));
    assert!(rig.herdr.running());
    rig.herdr_line(
        &herdr_snapshot(1, &[("t1", "live", Some("working"))]),
        at(301),
    );
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Running));
    assert_eq!(rig.herdr_connection().map(|c| c.ok), Some(true));
}

#[test]
fn recheck_restarts_hc_events_and_forgets_the_streak() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    for i in 0..4 {
        let step = rig.send(rig.herdr.ended("boom"), at(i * 20));
        rig.send(T3Msg::Tick, step.next_wake);
    }
    assert_eq!(rig.herdr_connection().map(|c| c.ok), Some(false));
    rig.send(T3Msg::Recheck, at(100));
    assert!(rig.herdr.running());
    let step = rig.send(rig.herdr.ended("boom"), at(101));
    assert_eq!(step.next_wake, at(102), "a new streak: 1 s");
}

#[test]
fn herdr_states_and_health_are_kept_for_the_next_start() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(0));
    rig.herdr_line(
        &herdr_snapshot(1, &[("t1", "live", Some("blocked"))]),
        at(1),
    );
    rig.worker.stop();
    assert!(!rig.herdr.running(), "stop ends the stream");

    let env = Env::new(
        cfg(rig.t3.dir(), rig.claude_dir.path()),
        rig.core.store.clone(),
        Hub::new(),
    );
    let hub = env.hub.clone();
    let (me, _r1) = channel();
    let (gh, _r2) = channel();
    let herdr = FakeHerdr::new();
    let mut again = T3Worker::new(
        env,
        Box::new(FakeT3::new(rig.t3.dir())),
        Box::new(rig.claude.clone()),
        Box::new(herdr.clone()),
        Box::new(FakeWatch::new()),
        me,
        gh,
        rig.core.open(),
    )
    .unwrap();
    let v = hub.snapshot().threads[&herdr_key("t1")].clone();
    assert_eq!(
        v.state,
        ThreadState::NeedsMe,
        "a needs_me row with no request"
    );
    assert!(v.stale, "stale until Herdr is read again");
    let row = hub
        .connections()
        .into_iter()
        .find(|c| c.name == "Herdr")
        .unwrap();
    assert_eq!((row.ok, row.state.as_str()), (true, "Checking"));
    again.start(at(10));
    assert!(
        hub.snapshot().threads[&herdr_key("t1")].stale,
        "the stream's started, but nothing's read yet"
    );
    let msg = herdr
        .line(&herdr_snapshot(1, &[("t1", "live", Some("working"))]))
        .unwrap();
    again.handle(msg, at(11));
    let v = hub.snapshot().threads[&herdr_key("t1")].clone();
    assert_eq!((v.state, v.stale), (ThreadState::Running, false));
}

#[test]
fn a_herdr_state_change_rechecks_held_finishes() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(0));
    while rig.gh.try_recv().is_ok() {}
    rig.herdr_line(
        &herdr_snapshot(1, &[("t1", "live", Some("working"))]),
        at(1),
    );
    assert!(matches!(rig.gh.try_recv(), Ok(GhMsg::Holds)));
    rig.herdr_line(&status_event(2, "t1", Some("working")), at(2));
    assert!(rig.gh.try_recv().is_err(), "no change, no recheck");
    rig.herdr_line(&status_event(3, "t1", Some("done")), at(3));
    assert!(matches!(rig.gh.try_recv(), Ok(GhMsg::Holds)));
}

#[test]
fn streams_that_deliver_their_snapshot_never_add_up_to_broken() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1"], at(0));
    let mut now = at(1);
    for seq in 1..=6 {
        rig.herdr_line(
            &herdr_snapshot(seq, &[("t1", "live", Some("working"))]),
            now,
        );
        rig.send(rig.herdr.ended("hc events stopped"), now + ms(10));
        // Each end is a first failure again: restarted 1 s later.
        now += Duration::from_secs(1) + ms(10);
        rig.send(T3Msg::Tick, now);
        assert!(rig.herdr.running(), "restarted after 1 s (round {seq})");
    }
    let row = rig.herdr_connection().unwrap();
    assert_eq!((row.ok, row.state.as_str()), (true, "Connected"));
}

#[test]
fn an_odd_terminal_in_the_snapshot_doesnt_stop_the_stream() {
    let (mut rig, _) = Rig::new(&[]);
    rig.start(t0());
    rig.link_terminals(&["t1", "vps/t7"], at(0));
    let line = r#"{"seq":1,"terminals":[{"address":"vps/t7","state":"lost","status":{"app":"x"}},{"address":"t1","state":"live","status":{"state":"working"}}]}"#;
    let msg = rig.herdr.line(line).unwrap();
    assert!(matches!(
        msg,
        T3Msg::Herdr {
            feed: airboss_sync::herdr::HerdrFeed::Update(_),
            ..
        }
    ));
    rig.send(msg, at(1));
    assert!(rig.herdr.running());
    assert_eq!(rig.herdr_state("t1"), Some(ThreadState::Running));
    assert_eq!(rig.herdr_state("vps/t7"), Some(ThreadState::Gone));
    assert_eq!(rig.herdr_connection().map(|c| c.ok), Some(true));
}
