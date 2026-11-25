//! SPEC-mcp performance targets. Run with:
//! `cargo test -p airboss-mcp --release -- --ignored perf --nocapture`
//!
//! Each check drives the real binary against a file-backed database with about 2,000
//! tasks and prints the machine next to the numbers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use airboss_core::{Actor, NewTask, RepoId, Store, TaskId};
use common::{Mcp, TempDir};
use serde_json::json;

fn machine() -> String {
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    format!(
        "{} | {} | macOS {}",
        run("sysctl", &["-n", "hw.model"]),
        run("sysctl", &["-n", "machdep.cpu.brand_string"]),
        run("sw_vers", &["-productVersion"]),
    )
}

fn p95(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[(samples.len() * 95).div_ceil(100) - 1]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn report(name: &str, samples: Vec<Duration>, target: Duration) {
    let runs = samples.len();
    let p = p95(samples);
    println!(
        "{name}: p95 {:.2} ms over {runs} runs (target < {} ms) on {}",
        ms(p),
        target.as_millis(),
        machine()
    );
    assert!(
        p < target,
        "{name}: p95 {p:?} is over the {target:?} target"
    );
}

/// 20 top-level tasks, 10 children each, 9 grandchildren each: 2,020 tasks, with repos
/// on the top level. Returns a grandchild to update.
fn big_tree(db: &Path) -> TaskId {
    let mut s = Store::open(db).unwrap();
    let repos: Vec<RepoId> = ["web", "api", "ios"]
        .iter()
        .map(|name| {
            let path = std::path::PathBuf::from(format!("/src/{name}"));
            s.add_repo(name, &path, None, &Actor::Me).unwrap().id
        })
        .collect();
    let mut leaf = TaskId(0);
    for a in 0..20 {
        let top = s
            .add_task(
                NewTask::new(format!("Top {a}"))
                    .in_repo(repos[a % 3])
                    .in_repo(repos[(a + 1) % 3]),
                &Actor::Me,
            )
            .unwrap()
            .id;
        for b in 0..10 {
            let mid = s
                .add_task(NewTask::new(format!("Mid {a}.{b}")).under(top), &Actor::Me)
                .unwrap()
                .id;
            for c in 0..9 {
                leaf = s
                    .add_task(
                        NewTask::new(format!("Leaf {a}.{b}.{c}")).under(mid),
                        &Actor::Me,
                    )
                    .unwrap()
                    .id;
            }
        }
    }
    leaf
}

#[test]
#[ignore]
fn perf_startup_to_initialize() {
    let dir = TempDir::new();
    big_tree(&dir.db());
    let samples: Vec<Duration> = (0..50)
        .map(|_| {
            let start = Instant::now();
            let mut mcp = Mcp::start(&dir.db(), Some("perf"));
            mcp.initialize();
            let took = start.elapsed();
            assert!(mcp.finish());
            took
        })
        .collect();
    report(
        "airboss mcp start to initialize",
        samples,
        Duration::from_millis(50),
    );
}

#[test]
#[ignore]
fn perf_tool_calls() {
    let dir = TempDir::new();
    let leaf = big_tree(&dir.db());
    let mut mcp = Mcp::start(&dir.db(), Some("perf"));
    mcp.initialize();
    let mut time = |tool: &str, args: serde_json::Value| {
        (0..300)
            .map(|_| {
                let start = Instant::now();
                let (is_error, text) = mcp.call(tool, args.clone());
                let took = start.elapsed();
                assert!(!is_error, "{text}");
                took
            })
            .collect::<Vec<_>>()
    };
    let list = time("list_tasks", json!({}));
    let list_filtered = time("list_tasks", json!({"repos": ["web"], "query": "leaf 1"}));
    let update = time("update_task", json!({"id": leaf.0, "note": "progress"}));
    let get = time("get_task", json!({"id": 1}));
    let target = Duration::from_millis(20);
    report("list_tasks on 2,020 tasks", list, target);
    report("list_tasks with repos and query", list_filtered, target);
    report("update_task (note)", update, target);
    report("get_task on a parent with 10 subtasks", get, target);
}

#[test]
#[ignore]
fn perf_idle_memory() {
    let dir = TempDir::new();
    big_tree(&dir.db());
    let mut mcp = Mcp::start(&dir.db(), Some("perf"));
    mcp.initialize();
    mcp.ok("list_tasks", json!({}));
    std::thread::sleep(Duration::from_millis(500));
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &mcp.pid().to_string()])
        .output()
        .unwrap();
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    let mb = kb as f64 / 1024.0;
    println!(
        "airboss mcp idle memory: {mb:.1} MB resident (target < 15 MB) on {}",
        machine()
    );
    assert!(mb < 15.0, "idle memory {mb:.1} MB is over 15 MB");
    assert!(mcp.finish());
}

#[test]
#[ignore]
fn perf_binary_size() {
    let dir = TempDir::new();
    let bin = Path::new(env!("CARGO_BIN_EXE_airboss"));
    let copy = dir.0.join("airboss");
    std::fs::copy(bin, &copy).unwrap();
    let status = std::process::Command::new("strip")
        .arg(&copy)
        .status()
        .unwrap();
    assert!(status.success());
    let size = |p: &Path| std::fs::metadata(p).unwrap().len() as f64 / (1024.0 * 1024.0);
    let (built, stripped) = (size(bin), size(&copy));
    println!(
        "airboss binary: {stripped:.2} MB stripped, {built:.2} MB as built (target < 10 MB) on {}",
        machine()
    );
    assert!(
        stripped < 10.0,
        "stripped binary {stripped:.2} MB is over 10 MB"
    );
}

#[test]
#[ignore]
fn perf_cli_ls() {
    let dir = TempDir::new();
    big_tree(&dir.db());
    let samples: Vec<Duration> = (0..50)
        .map(|_| {
            let start = Instant::now();
            let out = common::run(&dir.db(), &["ls"]);
            let took = start.elapsed();
            assert!(out.status.success());
            took
        })
        .collect();
    report(
        "airboss ls on 2,020 tasks, end to end",
        samples,
        Duration::from_millis(100),
    );
}
