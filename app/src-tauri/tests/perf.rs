//! Timings at 2,000 tasks and about 1,000 dependencies (`#[ignore]`; run with
//! `cargo test -p airboss-app --release --test perf -- --ignored --nocapture`).
//!
//! Budgets (SPEC-app "The view model and patches"): `snapshot()` 6 ms, build and diff 3 ms,
//! `catch_up()` under 10 ms, a write under 15 ms, a service rebuild under 4 ms; each the
//! 95th percentile of 200 runs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use airboss_app::Caller;
use airboss_app::commands::tasks;
use airboss_app::viewmodel::{ViewModel, diff};
use airboss_core::{Actor, Status, Store, TaskEdit};
use airboss_services::NeedsKind;
use common::{TestApp, needs_item, temp_dir};

const RUNS: usize = 200;

fn p95(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[(v.len() * 95 / 100).min(v.len() - 1)]
}

fn report(name: &str, v: Vec<Duration>, budget_ms: f64) -> Duration {
    let p = p95(v);
    let ms = p.as_secs_f64() * 1000.0;
    eprintln!("{name:<32} p95 {ms:>7.2} ms   (budget {budget_ms} ms)");
    p
}

#[test]
#[ignore = "perf"]
fn perf_rebuild_diff_and_write_at_2000_tasks() {
    let db = temp_dir("perf").join("airboss.db");
    let mut store = Store::open(&db).unwrap();
    let ids = airboss_app::seed::large(&mut store, 2000, 1000).unwrap();
    let edges = store.dependency_edges().unwrap().len();
    eprintln!("seeded {} tasks, {edges} dependencies", ids.len());
    let app = TestApp::with_store(store);
    app.sync_vm();

    // snapshot()
    let mut snap = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let s = app.core(|s| s.snapshot().unwrap());
        snap.push(t.elapsed());
        std::hint::black_box(s);
    }
    let snap_p = report("core.snapshot()", snap, 6.0);

    // Build rows and diff, from one snapshot.
    let snapshot = Arc::new(app.core(|s| s.snapshot().unwrap()));
    let repos = app.core(|s| s.repos().unwrap());
    let old = app.state.vm(|vm| vm.clone());
    let mut build = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let vm = ViewModel::build(
            snapshot.clone(),
            repos.clone(),
            old.sync.clone(),
            old.needs.clone(),
        );
        std::hint::black_box(diff(&old, &vm));
        build.push(t.elapsed());
    }
    let build_p = report("build rows + diff", build, 3.0);

    // A full catch_up() after a write from "another" handle (events_after, repos, snapshot,
    // services told, rebuild, diff).
    let mut catch = Vec::new();
    for i in 0..RUNS {
        let id = ids[i % ids.len()];
        app.core(|s| {
            s.edit_task(
                id,
                TaskEdit::default().title(format!("Renamed {i}")),
                &Actor::Me,
            )
        })
        .unwrap();
        let t = Instant::now();
        app.state.catch_up().unwrap();
        catch.push(t.elapsed());
    }
    let catch_p = report("catch_up() after a write", catch, 10.0);

    // A write command, returned to the UI (core write + catch-up + patch).
    let mut write = Vec::new();
    for i in 0..RUNS {
        let id = ids[(i * 3 + 1) % ids.len()];
        let to = if i % 2 == 0 {
            Status::Doing
        } else {
            Status::Todo
        };
        let t = Instant::now();
        let _ = tasks::set_status(&app.state, Caller::Main, id, to, None);
        write.push(t.elapsed());
    }
    let write_p = report("set_status, returned", write, 15.0);

    // A service bump: rebuild from the last snapshot and the services' new ones.
    let mut bump = Vec::new();
    for i in 0..RUNS {
        let items = if i % 2 == 0 {
            vec![needs_item(
                "q",
                NeedsKind::Question,
                ids[i % ids.len()],
                common::NOW,
            )]
        } else {
            Vec::new()
        };
        app.attention.set_items(items);
        let t = Instant::now();
        app.state.catch_up_services();
        bump.push(t.elapsed());
    }
    let bump_p = report("rebuild on a service bump", bump, 4.0);

    let bootstrap = Instant::now();
    let b = airboss_app::commands::detail::bootstrap(&app.state, Caller::Main).unwrap();
    let json = serde_json::to_vec(&b).unwrap();
    eprintln!(
        "bootstrap: {} rows, {} KB of JSON, {:.2} ms",
        b.rows.len(),
        json.len() / 1024,
        bootstrap.elapsed().as_secs_f64() * 1000.0
    );

    // Generous ceilings, so the test still means something on a slower Mac; the budgets
    // above are what's reported.
    assert!(snap_p < Duration::from_millis(30));
    assert!(build_p < Duration::from_millis(15));
    assert!(catch_p < Duration::from_millis(50));
    assert!(write_p < Duration::from_millis(60));
    assert!(bump_p < Duration::from_millis(20));
}
