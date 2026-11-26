//! Building `Launch`: where its tables go, the worker's first jobs, and the fakes it runs on.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use airboss_core::Store;
use airboss_launch::fake::{FakeGit, FakeHerdr, FakeT3Cli};
use airboss_launch::{CoreAccess, Git, LaunchConfig, LaunchMsg, LaunchTools, T3Cli};
use airboss_services::ErrorCode;
use airboss_services::fakes::{FakeClock, FakeSync};
use common::{H, TempDir, t0};

fn tools() -> LaunchTools {
    LaunchTools {
        t3: Box::new(FakeT3Cli::new()),
        git: Box::new(FakeGit::new()),
        herdr: Box::new(FakeHerdr::new()),
        clock: Arc::new(FakeClock::at(t0())),
    }
}

#[test]
fn an_in_memory_core_needs_a_file_for_launchs_tables() {
    let dir = TempDir::new();
    let core = CoreAccess::new(Arc::new(Mutex::new(Store::open_in_memory().unwrap())));
    let cfg = LaunchConfig::defaults(dir.path());
    let err = airboss_launch::open(cfg, core, Arc::new(FakeSync::new()), tools())
        .err()
        .unwrap();
    assert_eq!(err.code, ErrorCode::Storage);
}

#[test]
fn launchs_tables_go_in_cores_file_by_default() {
    let dir = TempDir::new();
    let path = dir.path().join("airboss.db");
    let core = CoreAccess::new(Arc::new(Mutex::new(Store::open(&path).unwrap())));
    let cfg = LaunchConfig::defaults(dir.path());
    airboss_launch::open(cfg, core, Arc::new(FakeSync::new()), tools()).unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    let n: i64 = c
        .query_row("SELECT count(*) FROM launch_threads", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn the_defaults_are_the_macs_paths() {
    let home = std::path::Path::new("/Users/someone");
    let cfg = LaunchConfig::defaults(home);
    assert_eq!(cfg.t3_path, home.join(".t3/bin/t3"));
    assert_eq!(cfg.research_dir, home.join(".t3/scratch"));
    assert_eq!(cfg.timeout, Duration::from_secs(30));
    assert_eq!(cfg.settle_after, Duration::from_secs(180));
    assert_eq!(cfg.home, home);
    assert_eq!(cfg.db_path, None);
}

#[test]
fn the_worker_starts_by_checking_t3_and_settling() {
    let mut h = H::new();
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::CheckAvailability));
    assert_eq!(h.worker.try_next(), Some(LaunchMsg::Settle));
    assert_eq!(h.worker.try_next(), None);
}

#[test]
fn the_fake_t3_refuses_a_start_that_isnt_well_formed() {
    let mut t3 = FakeT3Cli::new();
    let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let t = Duration::from_secs(1);
    // No stdin, a relative project, no --json, an unknown command.
    for (a, stdin) in [
        (
            argv(&["thread", "start", "--json", "--title", "x", "/p", "-"]),
            None,
        ),
        (
            argv(&["thread", "start", "--json", "--title", "x", "p", "-"]),
            Some("brief"),
        ),
        (
            argv(&["thread", "start", "--title", "x", "/p", "-"]),
            Some("brief"),
        ),
        (argv(&["thread", "delete", "x"]), None),
    ] {
        let out = t3.run(&a, stdin, t).unwrap();
        assert_eq!(out.status, Some(2), "{a:?}");
    }
    let ok = t3
        .run(
            &argv(&["thread", "start", "--json", "--title", "x", "/p", "-"]),
            Some("brief"),
            t,
        )
        .unwrap();
    assert_eq!(ok.status, Some(0));
    assert!(ok.stdout.contains("thread-1"));
    assert_eq!(t3.calls().len(), 5);
    assert_eq!(t3.starts().len(), 4);
}

#[test]
fn the_fake_git_answers_from_its_script() {
    let mut git = FakeGit::new();
    let repo = std::path::Path::new("/src/web");
    assert_eq!(git.default_branch(repo).unwrap().as_deref(), Some("main"));
    git.set_default(repo, None);
    assert_eq!(git.default_branch(repo).unwrap(), None);
    assert!(!git.has_branch(repo, "b").unwrap());
    git.add_branch(repo, "b");
    assert!(git.has_branch(repo, "b").unwrap());
    assert!(git.valid_branch_name("ok").unwrap());
    assert!(!git.valid_branch_name("a@{b").unwrap());
    git.refuse_name("ok");
    assert!(!git.valid_branch_name("ok").unwrap());
}
