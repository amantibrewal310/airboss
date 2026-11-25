//! `start(…)` with its worker thread, and the rest of the service: availability, Add to T3,
//! shutdown, and a worker that survives a panic.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use airboss_core::{Actor, RepoId, Store};
use airboss_launch::fake::{FakeGit, FakeT3Cli, Reply, Version};
use airboss_launch::{CoreAccess, Launch, LaunchConfig, LaunchTools};
use airboss_services::fakes::{FakeClock, FakeSync};
use airboss_services::{Availability, ErrorCode, LaunchService, Service, SharedStore, StoreChange};
use common::{TempDir, do_it, t0};

struct Running {
    dir: TempDir,
    store: SharedStore,
    t3: FakeT3Cli,
    cfg: LaunchConfig,
    launch: Arc<Launch>,
}

fn running(t3_installed: bool) -> Running {
    let dir = TempDir::new();
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
    let t3 = FakeT3Cli::new();
    let mut cfg = LaunchConfig::defaults(&dir.dir("home"));
    cfg.t3_path = dir.path().join("t3-never-run");
    if t3_installed {
        std::fs::write(&cfg.t3_path, "").unwrap();
    }
    cfg.research_dir = dir.dir("research");
    cfg.db_path = Some(dir.path().join("launch.db"));
    let launch = airboss_launch::start(
        cfg.clone(),
        CoreAccess::new(store.clone()),
        Arc::new(FakeSync::new()),
        LaunchTools {
            t3: Box::new(t3.clone()),
            git: Box::new(FakeGit::new()),
            clock: Arc::new(FakeClock::at(t0())),
        },
    )
    .unwrap();
    Running {
        dir,
        store,
        t3,
        cfg,
        launch,
    }
}

/// Waits until `availability()` is no longer `Unknown` and isn't `seen`.
fn wait_for(launch: &Launch, until: impl Fn(&Availability) -> bool) -> Availability {
    let changes = launch.changes();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut count = changes.current();
    loop {
        let a = launch.availability();
        if until(&a) {
            return a;
        }
        assert!(Instant::now() < deadline, "availability stayed {a:?}");
        count = changes.wait_past(count, Duration::from_millis(200));
    }
}

#[test]
fn the_worker_checks_t3_on_start() {
    let r = running(true);
    let a = wait_for(&r.launch, |a| *a != Availability::Unknown);
    assert_eq!(
        a,
        Availability::Ready {
            version: "0.1.12".into()
        }
    );
    assert_eq!(r.t3.calls()[0].argv, ["--version"]);
    r.launch.shutdown();
}

#[test]
fn without_t3_the_buttons_say_why_and_check_again_finds_it() {
    let r = running(false);
    let a = wait_for(&r.launch, |a| *a != Availability::Unknown);
    assert!(matches!(&a, Availability::Missing { reason } if reason == airboss_launch::T3_MISSING));
    assert!(r.t3.calls().is_empty(), "no launcher, no process");
    let c = &r.launch.connections()[0];
    assert!(!c.ok);
    // Installed, then "Check again".
    std::fs::write(&r.cfg.t3_path, "").unwrap();
    r.launch.recheck();
    let a = wait_for(&r.launch, |a| matches!(a, Availability::Ready { .. }));
    assert_eq!(
        a,
        Availability::Ready {
            version: "0.1.12".into()
        }
    );
    r.launch.shutdown();
}

#[test]
fn shutdown_returns_within_a_second_and_then_nothing_starts() {
    let r = running(true);
    wait_for(&r.launch, |a| *a != Availability::Unknown);
    let began = Instant::now();
    r.launch.shutdown();
    assert!(began.elapsed() < Duration::from_secs(1));
    let mut s = r.store.lock().unwrap();
    let repo = s
        .add_repo("web", &r.dir.dir("web"), None, &Actor::Me)
        .unwrap();
    let t = s
        .add_task(airboss_core::NewTask::new("T").in_repo(repo.id), &Actor::Me)
        .unwrap();
    drop(s);
    let e = r.launch.start(&do_it(t.id)).unwrap_err();
    assert_eq!(e.code, ErrorCode::ShuttingDown);
    assert_eq!(
        r.launch.add_to_t3(repo.id).unwrap_err().code,
        ErrorCode::ShuttingDown
    );
    assert!(r.t3.starts().is_empty());
    // Twice is fine.
    r.launch.shutdown();
}

#[test]
fn a_panic_in_the_worker_leaves_it_running() {
    let r = running(true);
    wait_for(&r.launch, |a| *a != Availability::Unknown);
    let panicked = Arc::new(AtomicBool::new(false));
    let p = panicked.clone();
    r.t3.on_call(move |_| {
        if !p.swap(true, Ordering::SeqCst) {
            panic!("a test panic inside the worker");
        }
    });
    r.t3.set_version(Version::Prints("t3 v9.9.9\n".into()));
    r.launch.recheck();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !panicked.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // The next request is still served.
    r.launch.recheck();
    let a = wait_for(&r.launch, |a| {
        *a == Availability::Ready {
            version: "9.9.9".into(),
        }
    });
    assert_eq!(
        a,
        Availability::Ready {
            version: "9.9.9".into()
        }
    );
    r.launch.shutdown();
}

#[test]
fn store_changed_does_nothing_and_returns_at_once() {
    let r = running(true);
    let snapshot = Arc::new(r.store.lock().unwrap().snapshot().unwrap());
    r.launch.store_changed(&StoreChange {
        events: vec![],
        snapshot,
    });
    r.launch.shutdown();
}

#[test]
fn add_to_t3_runs_project_add_with_the_repos_name_and_path() {
    let r = running(true);
    let path = r.dir.dir("src/booking-api");
    let repo = r
        .store
        .lock()
        .unwrap()
        .add_repo("booking-api", &path, None, &Actor::Me)
        .unwrap();
    r.launch.add_to_t3(repo.id).unwrap();
    let call =
        r.t3.calls()
            .into_iter()
            .find(|c| c.argv[0] == "project")
            .unwrap();
    assert_eq!(
        call.argv,
        [
            "project",
            "add",
            "--title",
            "booking-api",
            path.to_str().unwrap()
        ]
    );
    assert_eq!(call.stdin, None);
    assert!(r.t3.starts().is_empty(), "Add to T3 starts no thread");
    r.launch.shutdown();
}

#[test]
fn add_to_t3_reports_t3s_errors() {
    let r = running(true);
    let path = r.dir.dir("src/web");
    let repo = r
        .store
        .lock()
        .unwrap()
        .add_repo("web", &path, None, &Actor::Me)
        .unwrap();
    r.t3.push_other(Reply::Exit(1, "No running T3 Code server uses /x".into()));
    assert_eq!(
        r.launch.add_to_t3(repo.id).unwrap_err().code,
        ErrorCode::T3NotRunning
    );
    r.t3.push_other(Reply::Missing);
    assert_eq!(
        r.launch.add_to_t3(repo.id).unwrap_err().code,
        ErrorCode::T3Missing
    );
    r.t3.push_other(Reply::Exit(1, "Path is already a project".into()));
    let e = r.launch.add_to_t3(repo.id).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::T3Failed, "Path is already a project")
    );
    assert_eq!(
        r.launch.add_to_t3(RepoId(999)).unwrap_err().code,
        ErrorCode::NotFound
    );
    std::fs::remove_dir_all(&path).unwrap();
    assert_eq!(
        r.launch.add_to_t3(repo.id).unwrap_err().code,
        ErrorCode::RepoMissing
    );
    r.launch.shutdown();
}

#[test]
fn a_repo_name_that_reads_as_a_flag_isnt_passed_as_the_title() {
    let r = running(true);
    let path = r.dir.dir("src/odd");
    let repo = r
        .store
        .lock()
        .unwrap()
        .add_repo("--json", &path, None, &Actor::Me)
        .unwrap();
    r.launch.add_to_t3(repo.id).unwrap();
    let call =
        r.t3.calls()
            .into_iter()
            .find(|c| c.argv[0] == "project")
            .unwrap();
    assert_eq!(
        call.argv,
        ["project", "add", "--title", "json", path.to_str().unwrap()]
    );
    r.launch.shutdown();
}

#[test]
fn dropping_launch_stops_its_worker() {
    let r = running(true);
    wait_for(&r.launch, |a| *a != Availability::Unknown);
    let began = Instant::now();
    drop(r.launch);
    assert!(began.elapsed() < Duration::from_secs(1));
}
