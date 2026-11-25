//! SPEC-core performance targets. Run with:
//! `cargo test -p airboss-core --release -- --ignored perf --nocapture`
//!
//! Each check times many runs on a file-backed database (WAL, synchronous = NORMAL)
//! and asserts the 95th percentile, printing the machine next to the numbers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use airboss_core::{Actor, NewTask, Status, Store, TaskId};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-perf-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Tiny deterministic generator, so runs are repeatable without the rand crate.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

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

fn report(name: &str, p: Duration, target: Duration, runs: usize) {
    println!(
        "{name}: p95 {:.3} ms over {runs} runs (target < {} ms) on {}",
        p.as_secs_f64() * 1000.0,
        target.as_millis(),
        machine()
    );
    assert!(
        p < target,
        "{name}: p95 {p:?} is over the {target:?} target"
    );
}

/// 20 top-level tasks, 10 children each, 9 grandchildren each: 2,020 tasks.
/// Then about 1,000 dependencies between grandchildren of different parents, always from a
/// later task to an earlier one, so none is a loop or an ancestor link.
fn big_tree(store: &mut Store) -> Vec<TaskId> {
    let mut leaves = vec![];
    for a in 0..20 {
        let top = store
            .add_task(NewTask::new(format!("Top {a}")), &Actor::Me)
            .unwrap();
        for b in 0..10 {
            let mid = store
                .add_task(
                    NewTask::new(format!("Mid {a}.{b}")).under(top.id),
                    &Actor::Me,
                )
                .unwrap();
            for c in 0..9 {
                let leaf = store
                    .add_task(
                        NewTask::new(format!("Leaf {a}.{b}.{c}")).under(mid.id),
                        &Actor::Me,
                    )
                    .unwrap();
                leaves.push(leaf.id);
            }
        }
    }
    let mut rng = Lcg(42);
    let mut added = 0;
    while added < 1000 {
        let i = rng.below(leaves.len());
        let j = rng.below(leaves.len());
        let (later, earlier) = if i > j { (i, j) } else { (j, i) };
        // Different mid-level parents (9 leaves each), so never siblings' shared ancestor issue.
        if later / 9 == earlier / 9 {
            continue;
        }
        if store
            .add_dependency(leaves[later], leaves[earlier], &Actor::Me)
            .is_ok()
        {
            added += 1;
        }
    }
    leaves
}

#[test]
#[ignore]
fn perf_tree_of_2000_tasks_with_1000_dependencies() {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("airboss.db")).unwrap();
    big_tree(&mut store);
    assert!(store.tree().unwrap().len() >= 2000);

    let runs = 300;
    let samples = (0..runs)
        .map(|_| {
            let t = Instant::now();
            let tree = store.tree().unwrap();
            let elapsed = t.elapsed();
            assert!(tree.iter().any(|n| n.waiting));
            elapsed
        })
        .collect();
    report("tree()", p95(samples), Duration::from_millis(10), runs);
}

#[test]
#[ignore]
fn perf_add_task() {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("airboss.db")).unwrap();
    let parent = store.add_task(NewTask::new("Parent"), &Actor::Me).unwrap();
    let runs = 1000;
    let samples = (0..runs)
        .map(|i| {
            let t = Instant::now();
            store
                .add_task(
                    NewTask::new(format!("Task {i}")).under(parent.id),
                    &Actor::Me,
                )
                .unwrap();
            t.elapsed()
        })
        .collect();
    report("add_task", p95(samples), Duration::from_millis(1), runs);
}

#[test]
#[ignore]
fn perf_set_status_with_its_event() {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("airboss.db")).unwrap();
    let task = store.add_task(NewTask::new("Flip"), &Actor::Me).unwrap();
    let runs = 1000;
    let samples = (0..runs)
        .map(|i| {
            let to = if i % 2 == 0 {
                Status::Doing
            } else {
                Status::Todo
            };
            let t = Instant::now();
            store.set_status(task.id, to, None, &Actor::Me).unwrap();
            t.elapsed()
        })
        .collect();
    report("set_status", p95(samples), Duration::from_millis(1), runs);
}

#[test]
#[ignore]
fn perf_add_dependency_with_loop_check_on_2000_tasks() {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("airboss.db")).unwrap();
    let leaves = big_tree(&mut store);
    let mut rng = Lcg(7);
    let mut samples = vec![];
    while samples.len() < 300 {
        let i = rng.below(leaves.len());
        let j = rng.below(leaves.len());
        let (later, earlier) = if i > j { (i, j) } else { (j, i) };
        if later / 9 == earlier / 9 {
            continue;
        }
        let t = Instant::now();
        let _ = store.add_dependency(leaves[later], leaves[earlier], &Actor::Me);
        samples.push(t.elapsed());
    }
    report(
        "add_dependency",
        p95(samples),
        Duration::from_millis(2),
        300,
    );
}

#[test]
#[ignore]
fn perf_run_order_200_subtasks_400_dependencies() {
    let dir = TempDir::new();
    let mut store = Store::open(&dir.0.join("airboss.db")).unwrap();
    let parent = store
        .add_task(NewTask::new("Big plan"), &Actor::Me)
        .unwrap();
    let subtasks: Vec<TaskId> = (0..200)
        .map(|i| {
            store
                .add_task(
                    NewTask::new(format!("Step {i}")).under(parent.id),
                    &Actor::Me,
                )
                .unwrap()
                .id
        })
        .collect();
    let mut rng = Lcg(3);
    let mut added = 0;
    while added < 400 {
        let i = 1 + rng.below(199);
        let j = rng.below(i);
        if store
            .add_dependency(subtasks[i], subtasks[j], &Actor::Me)
            .is_ok()
        {
            added += 1;
        }
    }

    let runs = 1000;
    let samples = (0..runs)
        .map(|_| {
            let t = Instant::now();
            let order = store.run_order(parent.id).unwrap();
            let elapsed = t.elapsed();
            assert_eq!(order.waves.iter().map(Vec::len).sum::<usize>(), 200);
            elapsed
        })
        .collect();
    report("run_order", p95(samples), Duration::from_millis(2), runs);
}
