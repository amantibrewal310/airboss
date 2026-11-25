//! The GitHub worker: mapping, the finish triggers and holds, reopening, PRs found on T3
//! threads, accounts, backoff and rate limits, and its timing, all with chosen times.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use airboss_core::{Actor, LinkKind, PrState, Status, TaskId};
use airboss_services::{Checks, PrRef, PrView, Refresh, Review};
use airboss_sync::fake::{
    FakeClaude, FakeFailure, FakeGh, FakePr, FakeT3, FakeWatch, T3Fixture, TempDir, fake_token,
};
use airboss_sync::gh::GhClient;
use airboss_sync::{Env, GhMsg, GhWorker, Hub, Step, StoreSummary, T3Msg, T3Worker};
use common::{Core, at, cfg, t0};

const FIVE_MIN: u64 = 300;

struct Rig {
    core: Core,
    hub: Arc<Hub>,
    worker: GhWorker,
    to_t3: Receiver<T3Msg>,
    /// What the T3 worker sends the GitHub worker.
    from_t3: Receiver<GhMsg>,
    t3: Option<(T3Fixture, T3Worker, TempDir)>,
}

fn merged(author: &str) -> FakePr {
    let mut p = FakePr::open(author);
    p.state = "MERGED";
    p
}

fn closed(author: &str) -> FakePr {
    let mut p = FakePr::open(author);
    p.state = "CLOSED";
    p
}

fn me_only() -> FakeGh {
    FakeGh::new().account("me", true).sees("me", "acme/web")
}

impl Rig {
    fn new(core: Core, gh: FakeGh) -> Rig {
        Rig::build(core, gh, None, |_| {})
    }

    /// With a T3 worker on the same store, over `fixture`, so finishes can be held.
    fn with_t3(core: Core, gh: FakeGh, fixture: T3Fixture) -> Rig {
        Rig::build(core, gh, Some(fixture), |_| {})
    }

    fn build(
        core: Core,
        gh: FakeGh,
        fixture: Option<T3Fixture>,
        tweak: impl FnOnce(&mut airboss_sync::SyncConfig),
    ) -> Rig {
        let hub = Hub::new();
        let claude_dir = TempDir::new();
        let t3_dir = fixture
            .as_ref()
            .map(|f| f.dir().to_path_buf())
            .unwrap_or_else(|| claude_dir.path().join("no-t3"));
        let mut c = cfg(&t3_dir, claude_dir.path());
        tweak(&mut c);
        let env = Env::new(c, core.store.clone(), hub.clone());
        let (t3_tx, to_t3) = channel();
        let (gh_tx, from_t3) = channel();
        let summary = StoreSummary::from_change(&core.change());
        let client = Arc::new(Mutex::new(GhClient::new(Box::new(gh.clone()))));
        let mut worker =
            GhWorker::new(env.clone(), client, t3_tx.clone(), summary.clone(), t0()).unwrap();
        worker.start(t0());
        let t3 = fixture.map(|f| {
            let mut w = T3Worker::new(
                env,
                Box::new(FakeT3::new(f.dir())),
                Box::new(FakeClaude::new(claude_dir.path())),
                Box::new(FakeWatch::new()),
                t3_tx,
                gh_tx,
                summary.open,
            )
            .unwrap();
            w.start(t0());
            (f, w, claude_dir)
        });
        let mut rig = Rig {
            core,
            hub,
            worker,
            to_t3,
            from_t3,
            t3,
        };
        rig.core.mark();
        rig
    }

    fn send(&mut self, msg: GhMsg, now: SystemTime) -> Step {
        let step = self.worker.handle(msg, now);
        assert!(step.next_wake > now);
        assert!(step.next_wake <= now + Duration::from_secs(15 * 60));
        step
    }

    /// The first pass, 3 s after start.
    fn first_pass(&mut self) -> Step {
        self.send(GhMsg::Tick, at(3))
    }

    fn follow(&mut self, now: SystemTime) {
        let s = self.core.summary();
        if let Some((_, w, _)) = self.t3.as_mut() {
            w.handle(T3Msg::Store(s.clone()), now);
        }
        self.send(GhMsg::Store(s), now);
    }

    /// The T3 worker reads again, and whatever it tells the GitHub worker is delivered.
    fn t3_wal(&mut self, now: SystemTime) {
        let (_, w, _) = self.t3.as_mut().unwrap();
        w.handle(T3Msg::Refresh, now);
        self.pump(now);
    }

    /// Delivers what the T3 worker sent the GitHub worker.
    fn pump(&mut self, now: SystemTime) {
        while let Ok(msg) = self.from_t3.try_recv() {
            self.send(msg, now);
        }
    }

    fn pr(&self, r: &str) -> Option<PrView> {
        self.hub
            .snapshot()
            .prs
            .get(&PrRef::parse(r).unwrap())
            .cloned()
    }

    fn pr_state_in_core(&self, task: TaskId, r: &str) -> Option<PrState> {
        let links = self.core.lock().links(task).unwrap();
        links.iter().find(|l| l.r#ref == r).and_then(|l| l.pr_state)
    }

    fn github(&self) -> airboss_services::Connection {
        self.hub
            .connections()
            .into_iter()
            .find(|c| c.name == "GitHub")
            .unwrap()
    }

    fn status_events(&self, task: TaskId) -> Vec<(String, Option<String>)> {
        self.core
            .lock()
            .history(task)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == airboss_core::EventKind::Status)
            .map(|e| (e.actor.to_string(), e.body))
            .collect()
    }
}

fn task_with_pr(core: &Core, r: &str) -> TaskId {
    let t = core.task("Ship it");
    core.link_pr(t, r);
    t
}

// ---- mapping ----

#[test]
fn each_mapping_row_reaches_the_snapshot() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let mut p = FakePr::open("me");
    p.review = Some("APPROVED");
    p.rollup = Some("SUCCESS");
    p.additions = 120;
    p.deletions = 30;
    p.head = "feat/booking".into();
    let mut rig = Rig::new(core, me_only().pr("acme/web#1", p));
    rig.first_pass();
    let v = rig.pr("acme/web#1").unwrap();
    assert_eq!(v.state, Some(PrState::Open));
    assert_eq!((v.review, v.checks), (Review::Approved, Checks::Passing));
    assert_eq!((v.additions, v.deletions), (Some(120), Some(30)));
    assert_eq!(v.head_ref.as_deref(), Some("feat/booking"));
    assert_eq!(
        v.ready_since.as_deref(),
        Some(common::T0).map(|_| "2026-11-26T12:00:03.000Z")
    );
    assert!(v.linked && !v.stale);
    assert_eq!(rig.hub.snapshot().prs_for(t).len(), 1);
    assert!(rig.github().ok);
    assert_eq!(rig.github().state, "Signed in");
}

#[test]
fn ready_since_stays_while_ready_and_clears_for_a_draft() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    let first = rig.pr("acme/web#1").unwrap().ready_since;
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(rig.pr("acme/web#1").unwrap().ready_since, first);
    let mut d = FakePr::open("me");
    d.draft = true;
    gh.set_pr("acme/web#1", d);
    rig.send(GhMsg::Tick, at(3 + 2 * FIVE_MIN));
    let v = rig.pr("acme/web#1").unwrap();
    assert!(v.draft);
    assert_eq!(v.ready_since, None);
}

#[test]
fn a_linked_pr_not_read_yet_shows_with_no_state() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let rig = Rig::new(core, me_only());
    let v = rig.pr("acme/web#1").unwrap();
    assert_eq!(v.state, None);
}

// ---- finishing ----

#[test]
fn a_pr_merging_finishes_its_task_within_one_pass() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Todo);
    gh.set_state("acme/web#1", "MERGED");
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(rig.core.status(t), Status::Done);
    assert_eq!(
        rig.status_events(t),
        vec![(
            "sync:gh".to_string(),
            Some("PRs merged: acme/web#1".to_string())
        )]
    );
    assert_eq!(rig.pr_state_in_core(t, "acme/web#1"), Some(PrState::Merged));
}

#[test]
fn a_link_whose_first_read_is_merged_finishes_its_task() {
    let core = Core::new();
    let gh = me_only().pr("acme/web#2", merged("me"));
    let mut rig = Rig::new(core, gh);
    rig.first_pass();
    let t = task_with_pr(&rig.core, "acme/web#2");
    rig.follow(at(10));
    let step = rig.send(GhMsg::Tick, at(10));
    assert_eq!(step.next_wake, at(15), "a new PR link is read 5 s later");
    rig.send(GhMsg::Tick, at(15));
    assert_eq!(rig.core.status(t), Status::Done);
}

#[test]
fn a_link_made_already_merged_finishes_on_its_first_read() {
    let core = Core::new();
    let t = core.task("Ship it");
    core.lock()
        .link(
            t,
            airboss_core::NewLink::pr("acme/web#1").with_pr_state(PrState::Merged),
            &Actor::Me,
        )
        .unwrap();
    let mut rig = Rig::new(core, me_only().pr("acme/web#1", merged("me")));
    rig.first_pass();
    assert_eq!(
        rig.core.status(t),
        Status::Done,
        "no open state anywhere; the first read fires"
    );
}

#[test]
fn a_task_i_reopen_stays_open() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, me_only().pr("acme/web#1", merged("me")));
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Done);
    rig.core.set_status(t, Status::Doing);
    rig.follow(at(20));
    rig.send(GhMsg::Refresh(Refresh::PrsOf(t)), at(30));
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(rig.core.status(t), Status::Doing, "no trigger fires for it");
}

#[test]
fn one_merged_and_one_open_pr_finish_nothing() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link_pr(t, "acme/web#2");
    let mut rig = Rig::new(
        core,
        me_only()
            .pr("acme/web#1", merged("me"))
            .pr("acme/web#2", FakePr::open("me")),
    );
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Todo);
}

#[test]
fn only_closed_prs_finish_nothing() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, me_only().pr("acme/web#1", closed("me")));
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Todo);
    assert_eq!(rig.pr_state_in_core(t, "acme/web#1"), Some(PrState::Closed));
}

#[test]
fn a_finish_is_held_while_a_thread_runs_and_released_when_it_stops() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::T3Thread, "t-run");
    let mut rig = Rig::with_t3(
        core,
        me_only().pr("acme/web#1", merged("me")),
        T3Fixture::with_rows(),
    );
    rig.first_pass();
    assert_eq!(
        rig.core.status(t),
        Status::Todo,
        "held: an agent is still working"
    );
    assert_eq!(
        rig.pr_state_in_core(t, "acme/web#1"),
        Some(PrState::Merged),
        "core still learns the state"
    );
    let (f, _, _) = rig.t3.as_ref().unwrap();
    f.exec("UPDATE orchestration_v2_projection_runs SET status = 'completed', completed_at = '2026-11-26T12:01:00.000Z' WHERE run_id = 'r-run-2'");
    rig.t3_wal(at(60));
    assert_eq!(
        rig.core.status(t),
        Status::Done,
        "released when the thread stopped"
    );
}

#[test]
fn a_question_holds_a_finish_too() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::T3Thread, "t-question");
    let mut rig = Rig::with_t3(
        core,
        me_only().pr("acme/web#1", merged("me")),
        T3Fixture::with_rows(),
    );
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Todo);
}

#[test]
fn a_hold_is_dropped_when_the_pr_reads_open_again() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::T3Thread, "t-run");
    let gh = me_only().pr("acme/web#1", merged("me"));
    let mut rig = Rig::with_t3(core, gh.clone(), T3Fixture::with_rows());
    rig.first_pass();
    gh.set_state("acme/web#1", "OPEN");
    rig.send(GhMsg::Refresh(Refresh::PrsOf(t)), at(20));
    let (f, _, _) = rig.t3.as_ref().unwrap();
    f.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'completed' WHERE run_id = 'r-run-2'",
    );
    rig.t3_wal(at(60));
    assert_eq!(rig.core.status(t), Status::Todo, "the PR is open again");
}

#[test]
fn closed_then_reopened_goes_back_to_in_review() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link_pr(t, "acme/web#2");
    let gh = me_only()
        .pr("acme/web#1", merged("me"))
        .pr("acme/web#2", closed("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Done);
    // Reopened on GitHub: my own open PRs are read every pass, so it's seen.
    gh.set_state("acme/web#2", "OPEN");
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(rig.pr_state_in_core(t, "acme/web#2"), Some(PrState::Open));
    assert_eq!(rig.core.status(t), Status::InReview);
}

#[test]
fn a_pass_cut_short_between_steps_finishes_next_time() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    // The app stopped after core learned "merged" but before the task finished and before
    // observed_prs was written.
    rig.core
        .lock()
        .set_pr_state("acme/web#1", PrState::Merged, &Actor::SyncGh)
        .unwrap();
    gh.set_state("acme/web#1", "MERGED");
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(rig.core.status(t), Status::Done);
}

#[test]
fn a_merged_pr_i_link_to_a_second_task_finishes_it_too() {
    let core = Core::new();
    let a = task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, me_only().pr("acme/web#1", merged("me")));
    rig.first_pass();
    assert_eq!(rig.core.status(a), Status::Done);
    let b = task_with_pr(&rig.core, "acme/web#1");
    rig.follow(at(30));
    rig.send(GhMsg::Tick, at(35));
    assert_eq!(rig.core.status(b), Status::Done);
}

#[test]
fn a_thread_stopping_after_the_hold_releases_it_on_the_github_thread() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::T3Thread, "t-run");
    let mut rig = Rig::with_t3(
        core,
        me_only().pr("acme/web#1", merged("me")),
        T3Fixture::with_rows(),
    );
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Todo, "held");
    let (f, w, _) = rig.t3.as_mut().unwrap();
    f.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'completed' WHERE run_id = 'r-run-2'",
    );
    w.handle(T3Msg::Refresh, at(60));
    assert_eq!(
        rig.core.status(t),
        Status::Todo,
        "the T3 worker itself never finishes a task"
    );
    let msgs: Vec<GhMsg> = rig.from_t3.try_iter().collect();
    assert!(msgs.iter().any(|m| matches!(m, GhMsg::Holds)));
    for m in msgs {
        rig.send(m, at(61));
    }
    assert_eq!(rig.core.status(t), Status::Done);
}

#[test]
fn a_thread_stopping_before_the_merge_is_read_finishes_on_that_pass() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::T3Thread, "t-run");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::with_t3(core, gh.clone(), T3Fixture::with_rows());
    rig.first_pass();
    // The agent stops first: its hold re-check finds no hold yet and changes nothing.
    let (f, _, _) = rig.t3.as_ref().unwrap();
    f.exec(
        "UPDATE orchestration_v2_projection_runs SET status = 'completed' WHERE run_id = 'r-run-2'",
    );
    rig.t3_wal(at(30));
    assert_eq!(rig.core.status(t), Status::Todo);
    // Then GitHub says merged: no agent works on it, so the pass finishes it at once.
    gh.set_state("acme/web#1", "MERGED");
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(rig.core.status(t), Status::Done);
}

#[test]
fn a_missing_t3_database_holds_nothing() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::T3Thread, "t-run");
    let gh = me_only().pr("acme/web#1", merged("me"));
    let mut rig = Rig::with_t3(core, gh, T3Fixture::with_rows());
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Todo, "held while t-run runs");
    let (f, w, _) = rig.t3.as_mut().unwrap();
    std::fs::rename(f.dir().join("statev2.sqlite"), f.dir().join("moved.sqlite")).unwrap();
    // The hourly check reopens the database and finds it gone.
    w.handle(T3Msg::Tick, at(3700));
    rig.pump(at(3700));
    assert_eq!(
        rig.core.status(t),
        Status::Done,
        "its threads are gone, so the hold ends"
    );
}

#[test]
fn a_missing_claude_folder_holds_nothing() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link(t, LinkKind::ClaudeSession, "s-1");
    let gh = me_only().pr("acme/web#1", merged("me"));
    let mut rig = Rig::with_t3(core, gh, T3Fixture::with_rows());
    let claude_dir = rig.t3.as_ref().unwrap().2.path().to_path_buf();
    FakeClaude::new(&claude_dir).write(10, "s-1", "busy", "cli");
    let (_, w, _) = rig.t3.as_mut().unwrap();
    w.handle(T3Msg::ClaudeDir, at(1));
    rig.pump(at(1));
    rig.first_pass();
    assert_eq!(
        rig.core.status(t),
        Status::Todo,
        "held while the session is busy"
    );
    std::fs::remove_dir_all(&claude_dir).unwrap();
    let (_, w, _) = rig.t3.as_mut().unwrap();
    w.handle(T3Msg::ClaudeDir, at(60));
    rig.pump(at(60));
    assert_eq!(rig.core.status(t), Status::Done);
}

#[test]
fn the_t3_worker_gets_its_tick_even_when_refreshes_keep_pushing_the_timer() {
    let core = Core::new();
    let mut rig = Rig::new(core, me_only());
    rig.first_pass();
    while rig.to_t3.try_recv().is_ok() {}
    for i in 1..=12u64 {
        rig.send(GhMsg::Recheck, at(3 + 60 * i));
    }
    let ticks = rig
        .to_t3
        .try_iter()
        .filter(|m| matches!(m, T3Msg::Tick))
        .count();
    assert_eq!(
        ticks, 2,
        "one every 5 minutes over 12 minutes of on-demand passes"
    );
}

#[test]
fn a_broken_gh_says_so() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let gh = me_only();
    gh.break_auth_status();
    let mut rig = Rig::new(core, gh);
    rig.first_pass();
    let c = rig.github();
    assert!(!c.ok);
    assert_eq!(c.state, "Not working");
    assert!(c.fix.is_some());
}

// ---- PRs found on T3 threads ----

fn prs_fixture(core: &Core, task: TaskId) -> T3Fixture {
    let f = T3Fixture::with_rows();
    let linked = core.linked_at(task, "t-prs");
    let ts = |t: SystemTime| airboss_services::time::to_ts(t);
    let after = ts(linked + Duration::from_secs(60));
    let before = ts(linked - Duration::from_secs(86_400));
    f.exec(&format!(
        "UPDATE orchestration_v2_projection_threads SET payload_json =
           json_set(payload_json, '$.pullRequests[0].linkedAt', '{after}',
                                  '$.pullRequests[1].linkedAt', '{before}')
         WHERE thread_id = 't-prs'"
    ));
    f
}

#[test]
fn a_pr_on_a_thread_is_linked_only_after_github_read_it_with_githubs_state() {
    let core = Core::new();
    let t = core.task("Thread task");
    core.link(t, LinkKind::T3Thread, "t-prs");
    let f = prs_fixture(&core, t);
    let gh = me_only()
        .pr("acme/web#7", merged("me"))
        .pr("acme/web#3", merged("me"));
    let mut rig = Rig::with_t3(core, gh, f);
    let links = rig.core.lock().links(t).unwrap();
    assert!(
        links.iter().all(|l| l.kind != LinkKind::Pr),
        "not before GitHub read it"
    );
    rig.first_pass();
    let links = rig.core.lock().links(t).unwrap();
    let prs: Vec<_> = links.iter().filter(|l| l.kind == LinkKind::Pr).collect();
    assert_eq!(prs.len(), 1, "#3 was linked in T3 before our thread link");
    assert_eq!(prs[0].r#ref, "acme/web#7");
    assert_eq!(prs[0].linked_by, Actor::SyncT3);
    assert_eq!(prs[0].pr_state, Some(PrState::Merged), "GitHub's state");
    assert_eq!(
        prs[0].url.as_deref(),
        Some("https://github.com/acme/web/pull/7")
    );
    assert_eq!(
        rig.core.status(t),
        Status::Done,
        "its first read was merged"
    );
}

#[test]
fn a_pr_i_unlink_is_not_added_again() {
    let core = Core::new();
    let t = core.task("Thread task");
    core.link(t, LinkKind::T3Thread, "t-prs");
    let f = prs_fixture(&core, t);
    let mut rig = Rig::with_t3(core, me_only().pr("acme/web#7", FakePr::open("me")), f);
    rig.first_pass();
    let link = rig
        .core
        .lock()
        .links(t)
        .unwrap()
        .into_iter()
        .find(|l| l.kind == LinkKind::Pr)
        .unwrap();
    rig.core.lock().unlink(link.id, &Actor::Me).unwrap();
    rig.follow(at(10));
    rig.t3_wal(at(20));
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert!(
        rig.core
            .lock()
            .links(t)
            .unwrap()
            .iter()
            .all(|l| l.kind != LinkKind::Pr)
    );
}

#[test]
fn t3_saying_merged_while_github_says_open_changes_nothing() {
    let core = Core::new();
    let t = core.task("Thread task");
    core.link(t, LinkKind::T3Thread, "t-prs");
    let f = prs_fixture(&core, t);
    // T3's own snapshot of #7 says merged; GitHub says open.
    let mut rig = Rig::with_t3(core, me_only().pr("acme/web#7", FakePr::open("me")), f);
    rig.first_pass();
    assert_eq!(rig.pr_state_in_core(t, "acme/web#7"), Some(PrState::Open));
    assert_eq!(rig.core.status(t), Status::Todo);
}

// ---- accounts ----

fn two_accounts() -> FakeGh {
    FakeGh::new()
        .account("me", true)
        .account("work", false)
        .sees("me", "acme/web")
        .sees("work", "corp/secret")
        .pr("acme/web#1", FakePr::open("me"))
        .pr("corp/secret#4", FakePr::open("colleague"))
}

#[test]
fn the_second_account_is_used_only_after_not_found() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link_pr(t, "corp/secret#4");
    let gh = two_accounts();
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    let reads: Vec<(String, Vec<String>)> = gh
        .reads()
        .into_iter()
        .map(|c| (c.account.unwrap(), c.prs))
        .collect();
    assert_eq!(
        reads,
        vec![
            (
                "me".into(),
                vec!["acme/web#1".into(), "corp/secret#4".into()]
            ),
            ("work".into(), vec!["corp/secret#4".into()]),
        ]
    );
    assert_eq!(rig.pr("corp/secret#4").unwrap().state, Some(PrState::Open));
    // Next time, the account that could read it goes first.
    gh.clear_calls();
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    let reads: Vec<(String, Vec<String>)> = gh
        .reads()
        .into_iter()
        .map(|c| (c.account.unwrap(), c.prs))
        .collect();
    assert_eq!(
        reads,
        vec![
            ("me".into(), vec!["acme/web#1".into()]),
            ("work".into(), vec!["corp/secret#4".into()])
        ]
    );
}

#[test]
fn a_pr_no_account_can_see_is_no_access() {
    let core = Core::new();
    task_with_pr(&core, "hidden/repo#1");
    let mut rig = Rig::new(core, two_accounts());
    rig.first_pass();
    assert_eq!(
        rig.pr("hidden/repo#1").unwrap().problem,
        Some(airboss_services::PrProblem::NoAccess)
    );
}

#[test]
fn one_account_rate_limited_while_the_other_keeps_reading() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    core.link_pr(t, "corp/secret#4");
    let gh = two_accounts();
    gh.budget("me", 150, "2026-11-26T13:00:00Z");
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    let c = rig.github();
    assert!(!c.ok);
    assert!(c.state.starts_with("me:"), "{}", c.state);
    assert!(c.fix.unwrap().contains("13:00 UTC"));
    gh.clear_calls();
    gh.set_state("corp/secret#4", "MERGED");
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    let accounts: Vec<String> = gh.reads().into_iter().map(|c| c.account.unwrap()).collect();
    assert_eq!(accounts, vec!["work"], "me waits for its reset");
    assert_eq!(
        rig.pr("corp/secret#4").unwrap().state,
        Some(PrState::Merged)
    );
    assert!(
        rig.pr("acme/web#1").unwrap().stale,
        "me's PRs show their last state, stale"
    );
    // After the reset, me reads again.
    gh.budget("me", 4999, "2026-11-26T14:00:00Z");
    gh.clear_calls();
    rig.send(
        GhMsg::Tick,
        airboss_services::time::from_ts("2026-11-26T13:00:30.000Z").unwrap(),
    );
    assert!(
        gh.reads()
            .iter()
            .any(|c| c.account.as_deref() == Some("me"))
    );
}

#[test]
fn a_secondary_limit_holds_an_account_for_fifteen_minutes() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    gh.fail_next("me", FakeFailure::Secondary);
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    gh.clear_calls();
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    rig.send(GhMsg::Tick, at(3 + 2 * FIVE_MIN));
    assert!(gh.reads().is_empty());
    rig.send(GhMsg::Tick, at(3 + 3 * FIVE_MIN));
    assert_eq!(gh.reads().len(), 1);
}

#[test]
fn backoff_is_per_account_and_doubles() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::new(core, gh.clone());
    gh.fail_next(
        "me",
        FakeFailure::Exit("error connecting to api.github.com".into()),
    );
    rig.first_pass();
    let c = rig.github();
    assert!(!c.ok);
    assert!(c.state.contains("Can't reach GitHub"));
    gh.clear_calls();
    rig.send(GhMsg::Refresh(Refresh::Prs), at(30));
    assert!(gh.reads().is_empty(), "inside the 1-minute backoff");
    gh.fail_next("me", FakeFailure::Exit("still down".into()));
    let step = rig.send(GhMsg::Tick, at(40));
    assert_eq!(step.next_wake, at(63), "woken when the backoff ends");
    rig.send(GhMsg::Tick, at(63));
    assert_eq!(gh.reads().len(), 1, "after the backoff");
    gh.clear_calls();
    let step = rig.send(GhMsg::Tick, at(120));
    assert!(
        gh.reads().is_empty(),
        "the second failure backs off 2 minutes"
    );
    assert_eq!(step.next_wake, at(183));
    rig.send(GhMsg::Recheck, at(130));
    assert_eq!(gh.reads().len(), 1, "Check again tries at once");
    assert!(rig.github().ok);
}

#[test]
fn a_signed_out_account_is_broken_and_its_token_fetched_again() {
    let core = Core::new();
    task_with_pr(&core, "corp/secret#4");
    let gh = two_accounts();
    let mut rig = Rig::new(core, gh.clone());
    gh.fail_next("work", FakeFailure::SignedOut);
    rig.first_pass();
    let c = rig.github();
    assert!(!c.ok);
    assert!(c.fix.unwrap().contains("work needs `gh auth login` again"));
    gh.clear_calls();
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    let kinds: Vec<&str> = gh.calls().iter().map(|c| c.kind).collect();
    assert!(
        kinds.contains(&"auth_status"),
        "accounts read again after exit 4"
    );
    assert!(kinds.contains(&"token"), "the token is fetched again");
    assert!(rig.github().ok);
}

#[test]
fn no_token_in_argv_tables_logs_or_debug() {
    let core = Core::new();
    let t = task_with_pr(&core, "corp/secret#4");
    let gh = two_accounts();
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    let token = fake_token("work");
    assert!(gh.calls().iter().any(|c| c.token_set), "a token was used");
    for c in gh.calls() {
        assert!(c.argv.iter().all(|a| !a.contains(&token)));
    }
    assert!(!format!("{:?}", rig.hub.snapshot()).contains(&token));
    assert!(!format!("{:?}", rig.hub.connections()).contains(&token));
    let dir = rig.core.dir.path().to_path_buf();
    // The database, its -wal and -shm: our tables and core's.
    for f in std::fs::read_dir(&dir).unwrap() {
        let Ok(bytes) = std::fs::read(f.unwrap().path()) else {
            continue;
        };
        assert!(
            !String::from_utf8_lossy(&bytes).contains(&token),
            "not in any database file"
        );
    }
    let _ = t;
}

// ---- gh missing or signed out ----

#[test]
fn without_gh_pr_sync_is_off_and_nothing_else_changes() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, FakeGh::new().missing());
    rig.first_pass();
    let c = rig.github();
    assert_eq!((c.ok, c.state.as_str()), (false, "Not installed"));
    assert_eq!(
        c.fix.as_deref(),
        Some("Install the GitHub CLI to sync pull requests.")
    );
    assert_eq!(rig.core.status(t), Status::Todo);
    assert!(rig.pr("acme/web#1").unwrap().stale);
}

#[test]
fn every_account_failing_is_signed_out_and_stays_off() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, FakeGh::new().broken_account("old"));
    rig.first_pass();
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    let c = rig.github();
    assert_eq!((c.ok, c.state.as_str()), (false, "Not signed in"));
}

#[test]
fn signed_out_says_so() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, FakeGh::new());
    rig.first_pass();
    let c = rig.github();
    assert_eq!((c.ok, c.state.as_str()), (false, "Not signed in"));
    assert_eq!(c.fix.as_deref(), Some("Run `gh auth login` in a terminal."));
}

// ---- my open PRs ----

#[test]
fn my_open_prs_are_offered_and_go_when_merged() {
    let core = Core::new();
    let mut mine = FakePr::open("me");
    mine.head = "feat/booking".into();
    let gh = me_only()
        .pr("acme/web#9", mine)
        .pr("acme/web#10", FakePr::open("someone"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    let snap = rig.hub.snapshot();
    let mine: Vec<String> = snap
        .my_open_prs()
        .iter()
        .map(|p| p.r#ref.to_string())
        .collect();
    assert_eq!(mine, vec!["acme/web#9"]);
    assert_eq!(snap.suggested_prs("feat/booking").len(), 1);
    gh.set_state("acme/web#9", "MERGED");
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert!(rig.hub.snapshot().my_open_prs().is_empty());
    assert!(rig.pr("acme/web#9").is_none());
}

// ---- recently finished tasks ----

#[test]
fn prs_of_tasks_finished_in_the_last_seven_days_stay_in_pr_tasks() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let mut rig = Rig::new(core, me_only().pr("acme/web#1", merged("me")));
    rig.first_pass();
    assert_eq!(rig.core.status(t), Status::Done);
    rig.follow(at(10));
    let pr = PrRef::parse("acme/web#1").unwrap();
    assert_eq!(rig.hub.snapshot().pr_tasks.get(&pr), Some(&vec![t]));
    // Eight days on (the task's events are from the real clock, near now).
    let later = SystemTime::now() + Duration::from_secs(8 * 24 * 3600);
    rig.send(GhMsg::Refresh(Refresh::Prs), later);
    assert_eq!(rig.hub.snapshot().pr_tasks.get(&pr), None);
}

// ---- timing ----

#[test]
fn the_first_pass_is_three_seconds_after_start_then_every_five_minutes() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::new(core, gh.clone());
    let step = rig.send(GhMsg::Tick, at(1));
    assert_eq!(step.next_wake, at(3));
    assert!(gh.reads().is_empty());
    let step = rig.first_pass();
    assert_eq!(gh.reads().len(), 1);
    assert_eq!(step.next_wake, at(3 + FIVE_MIN));
    assert!(
        matches!(rig.to_t3.try_recv(), Ok(T3Msg::Tick)),
        "the timer drives the T3 worker's checks"
    );
    let step = rig.send(GhMsg::Tick, at(100));
    assert_eq!(
        step.next_wake,
        at(3 + FIVE_MIN),
        "an early tick changes nothing"
    );
    assert_eq!(gh.reads().len(), 1);
}

#[test]
fn on_demand_reads_are_limited() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", merged("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    gh.clear_calls();
    rig.send(GhMsg::Refresh(Refresh::Prs), at(10));
    assert_eq!(gh.reads().len(), 1);
    let step = rig.send(GhMsg::Refresh(Refresh::All), at(20));
    assert_eq!(gh.reads().len(), 1, "at most once a minute");
    assert_eq!(step.next_wake, at(70));
    rig.send(GhMsg::Tick, at(70));
    assert_eq!(gh.reads().len(), 2);

    gh.clear_calls();
    rig.send(GhMsg::Refresh(Refresh::PrsOf(t)), at(100));
    assert_eq!(
        gh.reads()[0].prs,
        vec!["acme/web#1"],
        "merged PRs are read for PrsOf"
    );
    let step = rig.send(GhMsg::Refresh(Refresh::PrsOf(t)), at(105));
    assert_eq!(step.next_wake, at(110), "at most every 10 s per task");
    assert_eq!(gh.reads().len(), 1);
}

#[test]
fn merged_and_closed_prs_are_not_read_again() {
    let core = Core::new();
    task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", merged("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    gh.clear_calls();
    rig.send(GhMsg::Tick, at(3 + FIVE_MIN));
    assert_eq!(gh.reads().len(), 1, "only my own open PRs");
    assert!(gh.reads()[0].prs.is_empty());
}

#[test]
fn a_pr_found_on_a_thread_is_read_five_seconds_later() {
    let core = Core::new();
    let mut rig = Rig::new(core, me_only());
    rig.first_pass();
    let step = rig.send(
        GhMsg::Read(vec![PrRef::parse("acme/web#1").unwrap()]),
        at(10),
    );
    assert_eq!(step.next_wake, at(15));
}

#[test]
fn after_a_six_hour_sleep_woke_runs_the_overdue_pass_at_once() {
    let core = Core::new();
    let t = task_with_pr(&core, "acme/web#1");
    let gh = me_only().pr("acme/web#1", FakePr::open("me"));
    let mut rig = Rig::new(core, gh.clone());
    rig.first_pass();
    gh.set_state("acme/web#1", "MERGED");
    let step = rig.send(GhMsg::Woke, at(6 * 3600));
    assert!(step.changed);
    assert_eq!(rig.core.status(t), Status::Done);
    assert_eq!(step.next_wake, at(6 * 3600 + FIVE_MIN));
}

#[test]
fn no_wait_is_longer_than_fifteen_minutes() {
    let core = Core::new();
    let mut rig = Rig::build(core, me_only(), None, |c| {
        c.pr_every = Duration::from_secs(3600)
    });
    let step = rig.first_pass();
    assert_eq!(step.next_wake, at(3 + 15 * 60));
    let step = rig.send(GhMsg::Tick, at(3 + 15 * 60));
    assert_eq!(
        step.next_wake,
        at(3 + 30 * 60),
        "still not due; wake again in 15 minutes"
    );
}
