//! Performance checks, `#[ignore]`d. Run in release:
//!
//! ```sh
//! cargo test -p airboss-attention --release --test perf -- --ignored --nocapture
//! ```
//!
//! Each prints the machine, then the 50th and 95th percentiles.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use airboss_attention::{AttentionConfig, Hidden, compute, start};
use airboss_core::{Actor, NewTask, Status, Store};
use airboss_services::fakes::{FakeClock, FakeSync};
use airboss_services::{AttentionService, Checks, NeedsKey, RequestKind, Review, Service};
use common::*;

fn machine() -> String {
    let sh = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let mem: u64 = sh("sysctl", &["-n", "hw.memsize"]).parse().unwrap_or(0);
    format!(
        "{} · {} cores · {} GB · macOS {} · {}",
        sh("sysctl", &["-n", "machdep.cpu.brand_string"]),
        sh("sysctl", &["-n", "hw.ncpu"]),
        mem >> 30,
        sh("sw_vers", &["-productVersion"]),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    )
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i]
}

fn report(what: &str, mut times: Vec<Duration>) -> Duration {
    times.sort();
    let (p50, p95, max) = (
        percentile(&times, 0.5),
        percentile(&times, 0.95),
        *times.last().unwrap(),
    );
    println!("machine: {}", machine());
    println!(
        "{what}: p50 {:.3} ms · p95 {:.3} ms · max {:.3} ms · {} runs",
        p50.as_secs_f64() * 1e3,
        p95.as_secs_f64() * 1e3,
        max.as_secs_f64() * 1e3,
        times.len()
    );
    p95
}

/// 2,000 tasks: 100 top-level tasks, each with 4 subtasks of 4 subtasks (depth 3),
/// statuses and activity spread over 30 days; 1,000 edges; 50 linked threads; 100 PRs.
fn generated() -> (Inputs2, Vec<Hidden>) {
    let statuses = [
        Doing, Todo, InReview, Blocked, Done, Doing, Todo, Proposed, Dropped, Doing,
    ];
    let mut core = Core::new();
    let mut ids = Vec::new();
    let mut n = 0i64;
    let mut next = |parent: Option<i64>, core: Core| -> (Core, i64) {
        n += 1;
        let status = statuses[(n as usize * 7) % statuses.len()];
        let day = (n as u64 * 13) % 30;
        let hour = (n as u64 * 5) % 24;
        (core.task(n, parent, status, &ts(day, hour)), n)
    };
    for _ in 0..100 {
        let (c, top) = next(None, core);
        core = c;
        ids.push(top);
        for _ in 0..4 {
            let (c, mid) = next(Some(top), core);
            core = c;
            ids.push(mid);
            for _ in 0..3 {
                let (c, leaf) = next(Some(mid), core);
                core = c;
                ids.push(leaf);
            }
        }
    }
    // 100 × 17 = 1,700 so far; 300 more top-level tasks make 2,000.
    for _ in 0..300 {
        let (c, t) = next(None, core);
        core = c;
        ids.push(t);
    }
    assert_eq!(ids.len(), 2000);
    for i in 0..1000i64 {
        let task = 2 + (i * 37) % 1999;
        let on = 1 + (i * 11) % (task - 1).max(1);
        core = core.edge(task, on);
    }
    for i in 0..150i64 {
        let t = 1 + (i * 13) % 2000;
        core = core.ready(t, &ts(25 + (i as u64 % 5), 0));
    }
    let mut snap = core.build();
    snap.edges.dedup();

    let mut links = Links::new();
    for i in 0..50u64 {
        let state = [NeedsMe, Errored, Running, Idle, Settled][i as usize % 5];
        let mut t = thread(&format!("t{i}"), state);
        if state == NeedsMe {
            let kind = if i % 2 == 0 {
                RequestKind::Question
            } else {
                RequestKind::Approval
            };
            t.request = Some(request(&format!("r{i}"), kind, "Proceed?", &ts(28, i % 24)));
        }
        t.run_id = Some(format!("run{i}"));
        t.error = Some("tests failed".into());
        t.last_activity_at = Some(ts(20 + i % 10, 0));
        links = links.thread(t, &[1 + (i as i64 * 41) % 2000, 1 + (i as i64 * 43) % 2000]);
    }
    for i in 0..100u64 {
        let mut p = pr(i + 1);
        p.updated_at = Some(format!("2026-11-{:02}T10:00:00Z", 1 + i % 28));
        p.checks = [Checks::Passing, Checks::Failing, Checks::Pending][i as usize % 3];
        p.review = [Review::Required, Review::Approved][i as usize % 2];
        if i % 4 == 0 {
            p.state = Some(airboss_core::PrState::Merged);
        }
        links = links.pr(p, &[1 + (i as i64 * 19) % 2000]);
    }
    let sync = links.build();
    let inputs = inputs(snap, sync);

    // 200 hidden rows: half on live items, half long gone.
    let now = at(30, 0);
    let live = compute(&inputs, &[], now, &AttentionConfig::default()).live_keys;
    let mut hidden: Vec<Hidden> = live
        .iter()
        .take(100)
        .enumerate()
        .map(|(i, k)| Hidden {
            key: k.clone(),
            until: (i % 2 == 0).then(|| at(31, 0)),
        })
        .collect();
    hidden.extend((0..200 - hidden.len()).map(|i| Hidden {
        key: NeedsKey(format!("question:gone-{i}")),
        until: None,
    }));
    (Inputs2(inputs), hidden)
}

struct Inputs2(airboss_attention::Inputs);

#[test]
#[ignore]
fn perf_compute_at_2000_tasks() {
    let (Inputs2(inputs), hidden) = generated();
    let cfg = AttentionConfig::default();
    let now = at(30, 0);
    let first = compute(&inputs, &hidden, now, &cfg);
    println!(
        "inputs: {} tasks, {} edges, {} ready, {} threads, {} PRs, {} hidden; {} live, {} shown",
        inputs.core.tree.len(),
        inputs.core.edges.len(),
        inputs.core.ready.len(),
        inputs.sync.threads.len(),
        inputs.sync.prs.len(),
        hidden.len(),
        first.live_keys.len(),
        first.items.len()
    );
    assert!(
        first.items.len() > 50,
        "the generated data exercises every kind"
    );
    for _ in 0..50 {
        std::hint::black_box(compute(&inputs, &hidden, now, &cfg));
    }
    let times: Vec<Duration> = (0..1000)
        .map(|i| {
            let began = Instant::now();
            let out = compute(&inputs, &hidden, now + Duration::from_secs(i), &cfg);
            let took = began.elapsed();
            std::hint::black_box(out);
            took
        })
        .collect();
    let p95 = report("compute, 2,000 tasks", times);
    assert!(p95 < Duration::from_millis(5), "p95 {p95:?} over 5 ms");
}

/// A real store of 2,000 tasks in progress, for the service checks.
fn big_store() -> airboss_services::SharedStore {
    let mut store = Store::open_in_memory().unwrap();
    for i in 0..2000 {
        let t = store
            .add_task(NewTask::new(format!("Task {i}")), &Actor::Me)
            .unwrap();
        if i % 3 == 0 {
            store
                .set_status(t.id, Status::Doing, None, &Actor::Me)
                .unwrap();
        }
    }
    Arc::new(Mutex::new(store))
}

#[test]
#[ignore]
fn perf_dismiss_to_updated_list() {
    let sync = Arc::new(FakeSync::new());
    let clock = Arc::new(FakeClock::at(SystemTime::now() + DAY * 6));
    let att = start(AttentionConfig::default(), big_store(), sync, clock).unwrap();
    let keys: Vec<NeedsKey> = att.needs_me().items.iter().map(|i| i.key.clone()).collect();
    assert!(keys.len() >= 600, "{}", keys.len());
    let times: Vec<Duration> = keys
        .iter()
        .take(300)
        .map(|k| {
            let began = Instant::now();
            att.dismiss(k).unwrap();
            let took = began.elapsed();
            assert!(att.needs_me().items.iter().all(|i| &i.key != k));
            took
        })
        .collect();
    let p95 = report("dismiss to updated list, 2,000 tasks", times);
    assert!(p95 < Duration::from_millis(20), "p95 {p95:?} over 20 ms");
    att.shutdown();
}

#[test]
#[ignore]
fn perf_change_to_changes_bumped() {
    let sync = Arc::new(FakeSync::new());
    let att = start(
        AttentionConfig::default(),
        big_store(),
        sync.clone(),
        Arc::new(airboss_services::SystemClock),
    )
    .unwrap();
    let ids: Vec<i64> = (1..=40).collect();
    let times: Vec<Duration> = ids
        .iter()
        .map(|&task| {
            let mut t = thread("a1", NeedsMe);
            t.request = Some(request(
                &format!("r{task}"),
                RequestKind::Question,
                "Q?",
                &ts(0, 0),
            ));
            let snapshot = Links::new().thread(t, &[task]).build();
            let seen = att.changes().current();
            let began = Instant::now();
            sync.set_snapshot(snapshot);
            assert!(att.changes().wait_past(seen, Duration::from_secs(2)) > seen);
            began.elapsed()
        })
        .collect();
    let p95 = report(
        "sync change to changes() bumped (150 ms settle included)",
        times,
    );
    assert!(p95 < Duration::from_millis(200), "p95 {p95:?} over 200 ms");
    att.shutdown();
}
