//! `Sync` as the app uses it: real worker threads over fakes, a settable clock.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::sync::{Mutex, mpsc::channel};
use std::time::{Duration, Instant};

use airboss_core::{LinkKind, PrState, Status, Store};
use airboss_services::Clock;
use airboss_services::fakes::FakeClock;
use airboss_services::{
    ErrorCode, PrRef, Refresh, Service, SharedStore, Source, SyncService, SyncSnapshot, ThreadKey,
    ThreadState,
};
use airboss_sync::fake::{
    FIXTURE_ENVIRONMENT, FakeClaude, FakeFailure, FakeGh, FakeHerdr, FakePr, FakeT3, FakeT3Handle,
    FakeWatch, T3Fixture, TempDir,
};
use airboss_sync::{Sync, Tools, run_once, start};
use common::{Core, cfg, t0};

struct Rig {
    core: Core,
    t3: T3Fixture,
    _claude_dir: TempDir,
    gh: FakeGh,
    clock: Arc<FakeClock>,
    t3h: FakeT3Handle,
    sync: Arc<Sync>,
}

fn tools(
    f: &T3Fixture,
    claude: &TempDir,
    gh: &FakeGh,
    clock: &Arc<FakeClock>,
) -> (Tools, FakeT3Handle) {
    let t3 = FakeT3::new(f.dir());
    let h = t3.handle();
    (
        Tools {
            gh: Box::new(gh.clone()),
            t3: Box::new(t3),
            claude: Box::new(FakeClaude::new(claude.path())),
            herdr: Box::new(FakeHerdr::new()),
            watch: Box::new(FakeWatch::new()),
            clock: clock.clone(),
        },
        h,
    )
}

impl Rig {
    fn new(core: Core, gh: FakeGh) -> Rig {
        Rig::on(core, gh, T3Fixture::with_rows())
    }

    fn on(core: Core, gh: FakeGh, t3: T3Fixture) -> Rig {
        let claude = TempDir::new();
        let clock = Arc::new(FakeClock::at(t0()));
        let (tools, t3h) = tools(&t3, &claude, &gh, &clock);
        let sync = start(cfg(t3.dir(), claude.path()), core.store.clone(), tools).unwrap();
        Rig {
            core,
            t3,
            _claude_dir: claude,
            gh,
            clock,
            t3h,
            sync,
        }
    }

    /// Waits up to 3 s for the snapshot to satisfy `ok`.
    fn wait(&self, ok: impl Fn(&SyncSnapshot) -> bool) -> bool {
        let changes = self.sync.changes();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut seen = changes.current();
        loop {
            if ok(&self.sync.snapshot()) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            seen = changes.wait_past(seen, left.min(Duration::from_millis(100)));
        }
    }

    /// Moves the clock on and tells the service the Mac woke, so overdue work runs.
    fn advance(&self, secs: u64) {
        self.clock.advance(Duration::from_secs(secs));
        self.sync.woke();
    }

    fn wait_status(&self, task: airboss_core::TaskId, status: Status) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.core.status(task) == status {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.sync.shutdown();
    }
}

fn key(r: &str) -> ThreadKey {
    ThreadKey {
        source: Source::T3,
        r#ref: r.into(),
    }
}

fn me() -> FakeGh {
    FakeGh::new().account("me", true).sees("me", "acme/web")
}

#[test]
fn threads_are_read_at_start_and_prs_three_seconds_later() {
    let core = Core::new();
    let t = core.task("Work");
    core.link(t, LinkKind::T3Thread, "t-question");
    core.link_pr(t, "acme/web#1");
    let rig = Rig::new(core, me().pr("acme/web#1", FakePr::open("me")));
    assert!(rig.wait(|s| {
        s.threads
            .get(&key("t-question"))
            .is_some_and(|v| v.state == ThreadState::NeedsMe)
    }));
    assert!(rig.gh.reads().is_empty(), "GitHub waits 3 s");
    rig.advance(3);
    let pr = PrRef::parse("acme/web#1").unwrap();
    assert!(rig.wait(|s| {
        s.prs
            .get(&pr)
            .is_some_and(|p| p.state == Some(PrState::Open))
    }));
    let names: Vec<String> = rig.sync.connections().into_iter().map(|c| c.name).collect();
    assert_eq!(names, vec!["GitHub", "T3 Code"]);
}

#[test]
fn a_merge_finishes_the_task_after_the_next_timer_pass() {
    let core = Core::new();
    let t = core.task("Work");
    core.link_pr(t, "acme/web#1");
    let rig = Rig::new(core, me().pr("acme/web#1", FakePr::open("me")));
    rig.advance(3);
    assert!(rig.wait(|s| !s.prs.is_empty() && s.prs.values().all(|p| p.state.is_some())));
    rig.gh.set_state("acme/web#1", "MERGED");
    rig.advance(300);
    assert!(rig.wait_status(t, Status::Done));
}

#[test]
fn a_merge_is_seen_within_one_interval_after_the_clock_is_set_back_an_hour() {
    let core = Core::new();
    let t = core.task("Work");
    core.link_pr(t, "acme/web#1");
    let rig = Rig::new(core, me().pr("acme/web#1", FakePr::open("me")));
    rig.advance(3);
    assert!(rig.wait(|s| !s.prs.is_empty() && s.prs.values().all(|p| p.state.is_some())));
    rig.gh.set_state("acme/web#1", "MERGED");
    rig.clock.set(rig.clock.now() - Duration::from_secs(3600));
    rig.sync.woke();
    // Time passes in 10 s steps. Before the fix the merge waited until the clock caught
    // up again: over an hour.
    let mut waited = 0;
    while waited < 2 * 300 && rig.core.status(t) != Status::Done {
        rig.advance(10);
        waited += 10;
        let deadline = Instant::now() + Duration::from_millis(30);
        while Instant::now() < deadline && rig.core.status(t) != Status::Done {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    assert!(
        rig.wait_status(t, Status::Done),
        "not seen after {waited} s"
    );
    assert!(waited <= 2 * 300, "{waited} s");
}

#[test]
fn refresh_reads_prs_now() {
    let core = Core::new();
    let t = core.task("Work");
    core.link_pr(t, "acme/web#1");
    let rig = Rig::new(core, me().pr("acme/web#1", FakePr::open("me")));
    rig.advance(3);
    assert!(rig.wait(|s| s.prs.values().any(|p| p.state.is_some())));
    rig.gh.set_state("acme/web#1", "MERGED");
    rig.sync.refresh(Refresh::PrsOf(t));
    assert!(rig.wait_status(t, Status::Done));
}

#[test]
fn store_changed_follows_new_links() {
    let mut core = Core::new();
    core.mark();
    let rig = Rig::new(core, me());
    assert!(rig.wait(|s| !s.threads.is_empty()));
    let t = rig.core.task("Later");
    rig.core.link(t, LinkKind::T3Thread, "t-run");
    let c = rig.core.change();
    rig.sync.store_changed(&c);
    assert!(rig.wait(|s| s.threads_for(t).len() == 1));
}

#[test]
fn lookup_pr_answers_and_names_each_failure() {
    let core = Core::new();
    let gh = FakeGh::new()
        .account("me", true)
        .account("work", false)
        .sees("me", "acme/web")
        .sees("work", "corp/secret")
        .pr("acme/web#1", FakePr::open("me"))
        .pr("corp/secret#2", FakePr::open("colleague"));
    let rig = Rig::new(core, gh.clone());
    let pr = |s: &str| PrRef::parse(s).unwrap();
    let v = rig.sync.lookup_pr(&pr("acme/web#1")).unwrap();
    assert_eq!((v.state, v.linked), (Some(PrState::Open), false));
    let v = rig.sync.lookup_pr(&pr("corp/secret#2")).unwrap();
    assert_eq!(
        v.author.as_deref(),
        Some("colleague"),
        "the second account's private repo"
    );
    assert_eq!(
        rig.sync.lookup_pr(&pr("acme/web#9")).unwrap_err().code,
        ErrorCode::PrNotFound
    );
    assert_eq!(
        rig.sync.lookup_pr(&pr("nobody/sees#1")).unwrap_err().code,
        ErrorCode::NoAccess
    );
    gh.fail_next("me", FakeFailure::Secondary);
    gh.fail_next("work", FakeFailure::Secondary);
    assert_eq!(
        rig.sync.lookup_pr(&pr("acme/web#1")).unwrap_err().code,
        ErrorCode::RateLimited
    );
    gh.fail_next("me", FakeFailure::Exit("boom".into()));
    gh.fail_next("work", FakeFailure::Exit("boom".into()));
    assert_eq!(
        rig.sync.lookup_pr(&pr("acme/web#1")).unwrap_err().code,
        ErrorCode::GhFailed
    );
}

#[test]
fn lookup_pr_without_gh_or_signed_out() {
    let rig = Rig::new(Core::new(), FakeGh::new().missing());
    let pr = PrRef::parse("a/b#1").unwrap();
    assert_eq!(
        rig.sync.lookup_pr(&pr).unwrap_err().code,
        ErrorCode::GhMissing
    );
    let rig = Rig::new(Core::new(), FakeGh::new());
    assert_eq!(
        rig.sync.lookup_pr(&pr).unwrap_err().code,
        ErrorCode::SignedOut
    );
}

#[test]
fn find_thread_and_thread_url() {
    let rig = Rig::new(Core::new(), me());
    assert!(rig.wait(|s| !s.projects.is_empty()));
    let found = rig
        .sync
        .find_thread(std::path::Path::new("/work/web"), "feat/run");
    assert_eq!(found, Some(key("t-web-new")));
    assert_eq!(
        rig.sync
            .find_thread(std::path::Path::new("/work/web"), "nope"),
        None
    );
    assert_eq!(
        rig.sync.thread_url(&key("mcp:launch:x")).as_deref(),
        Some(format!("t3code://threads/{FIXTURE_ENVIRONMENT}/mcp%3Alaunch%3Ax").as_str())
    );
    let claude = ThreadKey {
        source: Source::Claude,
        r#ref: "s".into(),
    };
    assert_eq!(rig.sync.thread_url(&claude), None);
}

#[test]
fn a_panic_in_a_worker_marks_t3_broken_and_the_thread_keeps_running() {
    let core = Core::new();
    let t = core.task("Work");
    core.link(t, LinkKind::T3Thread, "t-run");
    let rig = Rig::new(core, me());
    assert!(rig.wait(|s| s.threads.contains_key(&key("t-run"))));
    rig.t3h.panic_next();
    rig.sync.refresh(Refresh::T3);
    assert!(rig.wait(|s| s.threads.get(&key("t-run")).is_some_and(|v| v.stale)));
    let c = rig
        .sync
        .connections()
        .into_iter()
        .find(|c| c.name == "T3 Code")
        .unwrap();
    assert!(!c.ok);
    // Still alive: Check again recovers.
    rig.sync.recheck();
    assert!(rig.wait(|s| s.threads.get(&key("t-run")).is_some_and(|v| !v.stale)));
}

#[test]
fn the_snapshot_is_loaded_from_our_tables_at_start_and_stale_until_read() {
    let core = Core::new();
    let t = core.task("Work");
    core.link(t, LinkKind::T3Thread, "t-run");
    let store = core.store.clone();
    let rig = Rig::new(core, me());
    assert!(rig.wait(|s| s.threads.contains_key(&key("t-run"))));
    rig.sync.shutdown();
    // Again, with T3 failing to open: what was seen before shows, stale.
    let claude = TempDir::new();
    let clock = Arc::new(FakeClock::at(t0()));
    let (tools, h) = tools(&rig.t3, &claude, &rig.gh, &clock);
    h.fail_next(airboss_sync::t3::T3Error::Broken("layout changed".into()));
    let again = start(cfg(rig.t3.dir(), claude.path()), store, tools).unwrap();
    let snap = again.snapshot();
    let v = &snap.threads[&key("t-run")];
    assert_eq!(v.state, ThreadState::Running);
    assert!(v.stale);
    again.shutdown();
}

#[test]
fn shutdown_returns_quickly_and_later_calls_do_nothing() {
    let rig = Rig::new(Core::new(), me());
    let started = Instant::now();
    rig.sync.shutdown();
    assert!(started.elapsed() < Duration::from_secs(2));
    rig.sync.refresh(Refresh::All);
    rig.sync.woke();
    assert_eq!(
        rig.sync
            .find_thread(std::path::Path::new("/work/web"), "feat/run"),
        None
    );
    assert_eq!(
        rig.sync
            .lookup_pr(&PrRef::parse("a/b#1").unwrap())
            .unwrap_err()
            .code,
        ErrorCode::ShuttingDown
    );
}

#[test]
fn run_once_reads_everything_on_this_thread() {
    let core = Core::new();
    let t = core.task("Work");
    core.link(t, LinkKind::T3Thread, "t-question");
    core.link_pr(t, "acme/web#1");
    let f = T3Fixture::with_rows();
    let claude = TempDir::new();
    let gh = me().pr("acme/web#1", FakePr::open("me"));
    let clock = Arc::new(FakeClock::at(t0()));
    let (tools, _) = tools(&f, &claude, &gh, &clock);
    let snap = run_once(cfg(f.dir(), claude.path()), core.store.clone(), tools).unwrap();
    assert_eq!(snap.threads[&key("t-question")].state, ThreadState::NeedsMe);
    assert_eq!(
        snap.prs[&PrRef::parse("acme/web#1").unwrap()].state,
        Some(PrState::Open)
    );
}

#[test]
fn an_in_memory_store_is_refused() {
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
    let f = T3Fixture::with_rows();
    let claude = TempDir::new();
    let clock = Arc::new(FakeClock::at(t0()));
    let (tools, _) = tools(&f, &claude, &me(), &clock);
    let err = start(cfg(f.dir(), claude.path()), store, tools)
        .err()
        .unwrap();
    assert_eq!(err.code, ErrorCode::Storage);
    let _ = channel::<()>();
}

/// Polls `ok` for up to 3 s.
fn eventually(ok: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ok()
}

#[test]
fn herdr_lines_reach_the_snapshot_and_shutdown_stops_hc_events() {
    let core = Core::new();
    let t = core.task("Work");
    core.link(t, LinkKind::HerdrTerminal, "t4p");
    let f = T3Fixture::with_rows();
    let claude = TempDir::new();
    let clock = Arc::new(FakeClock::at(t0()));
    let (mut tools, _) = tools(&f, &claude, &me(), &clock);
    let herdr = FakeHerdr::new();
    tools.herdr = Box::new(herdr.clone());
    let sync = start(cfg(f.dir(), claude.path()), core.store.clone(), tools).unwrap();
    assert!(eventually(|| herdr.running()), "hc events starts");
    assert!(herdr.send(
        r#"{"channel":null,"seq":3,"terminals":[{"address":"t4p","state":"live","status":{"app":"claude-code","state":"blocked"},"status_ms":1795600000000,"title":"Fix it","cwd":"/w"}]}"#
    ));
    let herdr_key = ThreadKey {
        source: Source::Herdr,
        r#ref: "t4p".into(),
    };
    assert!(eventually(|| {
        sync.snapshot()
            .threads
            .get(&herdr_key)
            .is_some_and(|v| v.state == ThreadState::NeedsMe && !v.stale)
    }));
    assert_eq!(sync.snapshot().threads_for(t).len(), 1);
    let row = sync
        .connections()
        .into_iter()
        .find(|c| c.name == "Herdr")
        .unwrap();
    assert_eq!((row.ok, row.state.as_str()), (true, "Live state"));
    assert_eq!(sync.thread_url(&herdr_key), None);
    sync.shutdown();
    assert!(!herdr.running(), "no hc events child after shutdown");
    assert!(herdr.stops() >= 1);
}

#[test]
fn run_once_waits_for_herdr_s_snapshot() {
    let core = Core::new();
    let t = core.task("Work");
    core.link(t, LinkKind::HerdrTerminal, "t4p");
    let f = T3Fixture::with_rows();
    let claude = TempDir::new();
    let clock = Arc::new(FakeClock::at(t0()));
    let (mut tools, _) = tools(&f, &claude, &me(), &clock);
    let herdr = FakeHerdr::new();
    tools.herdr = Box::new(herdr.clone());
    // The stream's snapshot arrives a little after it starts, as hc's does.
    let feeder = {
        let herdr = herdr.clone();
        std::thread::spawn(move || {
            assert!(eventually(|| herdr.running()));
            herdr.send(
                r#"{"channel":null,"seq":3,"terminals":[{"address":"t4p","state":"live","status":{"state":"working"},"title":"","cwd":"/w"}]}"#,
            )
        })
    };
    let snap = run_once(cfg(f.dir(), claude.path()), core.store.clone(), tools).unwrap();
    assert!(feeder.join().unwrap());
    let key = ThreadKey {
        source: Source::Herdr,
        r#ref: "t4p".into(),
    };
    assert_eq!(snap.threads[&key].state, ThreadState::Running);
    assert!(!herdr.running(), "run_once leaves no child behind");
}
