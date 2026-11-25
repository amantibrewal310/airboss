//! One real Do it: the app's `do_it` command on the real `launch`, `sync` and `attention`,
//! with the real `t3`, `git`, `gh` and T3's database.
//!
//! Ignored: it creates a real T3 project and thread. Run it by hand, once, on a throwaway
//! repo and a temporary database (tests/e2e/real_launch.sh does the setup):
//!
//! ```text
//! AIRBOSS_DB=/tmp/qa/launch/airboss.db AIRBOSS_QA_TASK=1 \
//!   cargo test -p airboss-app --test real_launch -- --ignored --nocapture
//! ```
//!
//! It follows the app's own path: Do it; if T3 says the repo isn't a project, "Add to T3" and
//! Do it again. It prints `t3`'s raw output (stdout and stderr of every call), then checks the
//! thread link (`launch`), the branch, `doing`, sync's live state and `find_thread`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use airboss_app::commands::launch::{add_to_t3, do_it};
use airboss_app::dto::DoItResult;
use airboss_app::{AppConfig, AppState, Caller, RecordingShell, Services, share};
use airboss_core::{LinkKind, Status, Store, TaskId};
use airboss_launch::{
    CmdOutput, CoreAccess, LaunchConfig, LaunchTools, ProcessRunner, RealGit, T3Cli,
};
use airboss_services::{Availability, ErrorCode, SystemClock};

/// Every `t3` call, as run: argv, stdin length, and what came back.
type Calls = Arc<Mutex<Vec<(Vec<String>, usize, io::Result<CmdOutput>)>>>;

/// The real `t3`, recording each call.
struct Tee {
    real: ProcessRunner,
    calls: Calls,
}

impl T3Cli for Tee {
    fn run(
        &mut self,
        argv: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        let out = self.real.run(argv, stdin, timeout);
        let copy = match &out {
            Ok(o) => Ok(o.clone()),
            Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
        };
        self.calls
            .lock()
            .unwrap()
            .push((argv.to_vec(), stdin.map_or(0, str::len), copy));
        out
    }
}

fn print_calls(calls: &Calls, from: usize) {
    for (argv, stdin, out) in calls.lock().unwrap().iter().skip(from) {
        println!("t3 {}  (stdin: {stdin} bytes)", argv.join(" "));
        match out {
            Ok(o) => println!(
                "  exit {:?}\n  stdout: {}\n  stderr: {}",
                o.status,
                o.stdout.trim(),
                o.stderr.trim()
            ),
            Err(e) => println!("  failed to run: {e}"),
        }
    }
}

fn wait_for<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            println!("{what}: after {} ms", start.elapsed().as_millis());
            return v;
        }
        assert!(start.elapsed() < limit, "{what}: not within {limit:?}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
#[ignore = "creates a real T3 thread; run by hand once (tests/e2e/real_launch.sh)"]
fn one_real_do_it() {
    let db =
        PathBuf::from(std::env::var("AIRBOSS_DB").expect("set AIRBOSS_DB to a temporary file"));
    assert!(
        !db.to_string_lossy().contains("Application Support/Airboss"),
        "never the real database"
    );
    let task = TaskId(
        std::env::var("AIRBOSS_QA_TASK")
            .expect("set AIRBOSS_QA_TASK to the task linked to the throwaway repo")
            .parse()
            .unwrap(),
    );
    let home = airboss_app::services::home();
    let store = share(Store::open(&db).unwrap());

    // The services exactly as the app builds them, except that `t3` calls are recorded.
    let calls: Calls = Arc::default();
    let sync_cfg = airboss_sync::SyncConfig::defaults(&home);
    let sync: Arc<dyn airboss_services::SyncService> = airboss_sync::start(
        sync_cfg.clone(),
        store.clone(),
        airboss_sync::Tools::real(&sync_cfg),
    )
    .unwrap();
    let launch_cfg = LaunchConfig::defaults(&home);
    let tools = LaunchTools {
        t3: Box::new(Tee {
            real: ProcessRunner::new(
                launch_cfg.t3_path.clone(),
                launch_cfg.home.clone(),
                airboss_launch::child_env(std::env::vars_os()),
            ),
            calls: calls.clone(),
        }),
        git: Box::new(RealGit::new(&home)),
        clock: Arc::new(SystemClock),
    };
    let launch = airboss_launch::start(
        launch_cfg,
        CoreAccess::new(store.clone()),
        sync.clone(),
        tools,
    )
    .unwrap();
    let attention = airboss_attention::start(
        airboss_attention::AttentionConfig::default(),
        store.clone(),
        sync.clone(),
        Arc::new(SystemClock),
    )
    .unwrap();
    let services = Services {
        sync: sync.clone(),
        launch: launch.clone(),
        attention,
    };
    let shell = Arc::new(RecordingShell::new());
    let state = AppState::new(
        store.clone(),
        services,
        shell.clone(),
        Arc::new(SystemClock),
        AppConfig::new(Default::default(), None),
    )
    .unwrap();
    state.start_background();

    let version = wait_for("t3 available", Duration::from_secs(20), || {
        match airboss_services::LaunchService::availability(launch.as_ref()) {
            Availability::Ready { version } => Some(version),
            _ => None,
        }
    });
    println!("t3 version {version}");
    print_calls(&calls, 0);

    let (repo, repo_path) = {
        let s = store.lock().unwrap();
        let t = s.task(task).unwrap();
        let repos = s.repos().unwrap();
        let r = repos
            .into_iter()
            .find(|r| t.repos.contains(&r.id))
            .expect("the task is linked to a repo");
        (r.id, PathBuf::from(r.path))
    };
    let projects = sync.snapshot().projects.clone();
    let is_project = projects.iter().any(|p| repo_path.starts_with(&p.path));
    println!(
        "T3 projects: {}; the repo is {}a project",
        projects.len(),
        if is_project { "" } else { "not " }
    );

    // Do it, as the main window's button calls it.
    let before = calls.lock().unwrap().len();
    let first = do_it(&state, Caller::Main, task, None, None);
    print_calls(&calls, before);
    let report = match first {
        Ok(DoItResult::Started { report, .. }) => report,
        other => panic!("do_it: {other:?}"),
    };
    println!("first report: {report:?}");
    let report = if report.started.is_empty() {
        let (_, e) = &report.failed[0];
        assert_eq!(e.code, ErrorCode::NotAProject, "{e:?}");
        println!("not a project, as expected: {:?}", e.message);
        // The toast's "Add to T3", then the toast's Retry.
        let before = calls.lock().unwrap().len();
        add_to_t3(&state, Caller::Main, repo).unwrap();
        print_calls(&calls, before);
        let before = calls.lock().unwrap().len();
        let again = do_it(&state, Caller::Main, task, None, None).unwrap();
        print_calls(&calls, before);
        match again {
            DoItResult::Started { report, .. } => report,
            other => panic!("do_it again: {other:?}"),
        }
    } else {
        report
    };
    println!("report: {report:?}");
    assert_eq!(report.started.len(), 1, "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    let thread = report.started[0].key.clone();
    println!("THREAD {}", thread.r#ref);

    // core: the link, the branch and doing.
    let (links, t) = {
        let s = store.lock().unwrap();
        (s.links(task).unwrap(), s.task(task).unwrap())
    };
    let link = links
        .iter()
        .find(|l| l.kind == LinkKind::T3Thread)
        .expect("a thread link");
    println!("link: {} by {}", link.r#ref, link.linked_by);
    assert_eq!(link.r#ref, thread.r#ref);
    assert_eq!(link.linked_by.to_string(), "launch");
    let branch = t.branch.clone().expect("the branch is set");
    println!("branch: {branch}; status {:?}", t.status);
    assert_eq!(t.status, Status::Doing);

    // sync: the live state, and find_thread by project and branch (T3 records the branch once
    // the worktree is set up, after `thread start` returns).
    let view = wait_for("sync sees the thread", Duration::from_secs(60), || {
        sync.snapshot().threads.get(&thread).cloned()
    });
    println!("thread view: {view:?}");
    let found = wait_for("find_thread resolves it", Duration::from_secs(180), || {
        sync.find_thread(&repo_path, &branch)
    });
    println!("find_thread: {found:?}");
    assert_eq!(found, thread);
    let row = state.row(task).unwrap();
    println!("row live chip: {:?}; status {:?}", row.live, row.status);
    println!("thread url: {:?}", sync.thread_url(&thread));

    state.shutdown(Duration::from_secs(2));
}
