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
use airboss_sync::fake::{FakeClaude, FakeT3, FakeT3Handle, FakeWatch, T3Fixture, TempDir};
use airboss_sync::t3::T3Error;
use airboss_sync::{Env, GhMsg, Hub, Step, T3Msg, T3Worker, WatchTarget};
use common::{Core, at, cfg, ms, t0};

struct Rig {
    core: Core,
    t3: T3Fixture,
    claude_dir: TempDir,
    claude: FakeClaude,
    fake_t3: FakeT3Handle,
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
    assert_eq!(rig.t3_connection().state, "Live state");
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
    assert_eq!(c.state, "Live state broken since 12:00 UTC");
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
    assert_eq!(notes[1].1, "Run failed: Run failed", "no error item");
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
    let read: Vec<String> = match rig.gh.try_recv().unwrap() {
        GhMsg::Read(prs) => prs.into_iter().map(|p| p.to_string()).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        read,
        vec!["acme/web#7"],
        "github.com only, at or after the link, canonical"
    );
    rig.send(T3Msg::Refresh, at(60));
    assert!(
        rig.gh.try_recv().is_err(),
        "already waiting: not asked again"
    );
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
fn an_unknown_claude_status_shows_the_agents_row() {
    let (mut rig, _) = Rig::new(&[]);
    rig.claude.write(10, "s", "dreaming", "cli");
    rig.start(t0());
    let row = rig
        .hub
        .connections()
        .into_iter()
        .find(|c| c.name == "Agents")
        .unwrap();
    assert!(!row.ok);
    assert!(row.fix.unwrap().contains("dreaming"));
}

#[test]
fn no_claude_folder_means_no_agents_row() {
    let (mut rig, _) = Rig::new(&[]);
    std::fs::remove_dir_all(rig.claude_dir.path()).unwrap();
    rig.start(t0());
    assert!(rig.hub.connections().iter().all(|c| c.name != "Agents"));
}
