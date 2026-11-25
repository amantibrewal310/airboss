//! One T3 pass on the fixture grown to 2,000 threads, 50 of them linked, with 200 untracked
//! candidates. Run in release:
//!
//! ```sh
//! cargo test -p airboss-sync --release --test perf -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use airboss_sync::fake::T3Fixture;
use airboss_sync::t3::{T3Db, T3Query, T3Source};
use airboss_sync::threads;

#[test]
#[ignore = "performance; run in release by hand"]
fn a_t3_pass_on_2000_threads_is_under_5_ms_at_p95() {
    let f = T3Fixture::with_rows();
    f.exec(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000)
         INSERT INTO orchestration_v2_projection_threads
           (thread_id, project_id, title, default_provider, runtime_mode, interaction_mode,
            created_at, updated_at, payload_json)
         SELECT 'g-' || i, 'p-web', 'Generated ' || i, 'claudeAgent', 'full-access', 'default',
           strftime('%Y-%m-%dT%H:%M:%fZ', '2026-11-26T12:00:00Z', '-' || i || ' minutes'),
           strftime('%Y-%m-%dT%H:%M:%fZ', '2026-11-26T12:00:00Z', '-' || i || ' minutes'),
           json_object('branch', 'feat/g' || i, 'worktreePath', NULL,
             'lineage', json_object('parentThreadId', NULL, 'rootThreadId', 'g-' || i),
             'settledAt', NULL, 'pullRequests', json_array())
         FROM n;
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2000),
              r(i, o) AS (SELECT i, 1 FROM n UNION ALL SELECT i, 2 FROM n UNION ALL SELECT i, 3 FROM n)
         INSERT INTO orchestration_v2_projection_runs
           (run_id, thread_id, ordinal, provider, status, requested_at, completed_at, payload_json)
         SELECT 'g-' || i || '-r' || o, 'g-' || i, o, 'claudeAgent',
           CASE WHEN o = 3 AND i % 10 = 0 THEN 'running' ELSE 'completed' END,
           '2026-11-26T10:00:00.000Z', '2026-11-26T10:05:00.000Z', json_object('startedAt', '2026-11-26T10:00:01.000Z')
         FROM r;",
    );
    let linked: Vec<String> = (1..=50).map(|i| format!("g-{}", i * 7)).collect();
    let q = T3Query {
        linked: linked.clone(),
        untracked_since: Some("2026-11-19T12:00:00.000Z".into()),
        ..T3Query::default()
    };
    let mut db = T3Db::new(f.dir());
    db.open().unwrap();
    let mut times: Vec<Duration> = Vec::new();
    for _ in 0..200 {
        let t = Instant::now();
        db.data_version().unwrap();
        let read = db.read(&q).unwrap();
        let views = threads::linked(&read, &linked, &HashMap::new());
        let untracked = threads::untracked(read.untracked.as_ref().unwrap(), &Default::default());
        assert_eq!(views.views.len(), 50);
        assert_eq!(untracked.len(), 200);
        times.push(t.elapsed());
    }
    times.sort();
    let p95 = times[times.len() * 95 / 100];
    println!(
        "T3 pass, 50 linked of 2,020 threads, 200 untracked: median {:?}, p95 {:?}",
        times[times.len() / 2],
        p95
    );
    assert!(p95 < Duration::from_millis(5), "p95 {p95:?}");
}

/// One GitHub pass, 50 linked PRs, one account: one `gh` call, one rate-limit point, and
/// our share of the work (building the query, parsing 50 PRs, comparing, writing rows).
/// `FakeGh` builds its JSON answer inside the timed span, so this is an upper bound.
#[test]
#[ignore = "performance; run in release by hand"]
fn a_github_pass_on_50_prs_is_one_call_and_under_150_ms() {
    use std::sync::{Arc, Mutex, mpsc::channel};

    use airboss_core::{Actor, NewLink, NewTask, Store};
    use airboss_services::Refresh;
    use airboss_sync::fake::{FakeGh, FakePr, TempDir};
    use airboss_sync::gh::GhClient;
    use airboss_sync::{Env, GhMsg, GhWorker, Hub, StoreSummary, SyncConfig};

    let dir = TempDir::new();
    let store = Arc::new(Mutex::new(
        Store::open(&dir.path().join("airboss.db")).unwrap(),
    ));
    let mut gh = FakeGh::new().account("me", true).sees("me", "acme/web");
    for n in 1..=50 {
        let r = format!("acme/web#{n}");
        let mut s = store.lock().unwrap();
        let t = s
            .add_task(NewTask::new(format!("Task {n}")), &Actor::Me)
            .unwrap();
        s.link(t.id, NewLink::pr(&r), &Actor::Me).unwrap();
        gh = gh.pr(&r, FakePr::open("someone"));
    }
    let snapshot = store.lock().unwrap().snapshot().unwrap();
    let summary = StoreSummary::from_change(&airboss_services::StoreChange {
        events: Vec::new(),
        snapshot: Arc::new(snapshot),
    });
    let env = Env::new(SyncConfig::defaults(dir.path()), store, Hub::new());
    let (t3, _rx) = channel();
    let now = airboss_services::time::from_ts("2026-11-26T12:00:00.000Z").unwrap();
    let client = Arc::new(Mutex::new(GhClient::new(Box::new(gh.clone()))));
    let mut w = GhWorker::new(env, client, t3, summary, now).unwrap();
    w.start(now);
    let mut times = Vec::new();
    for i in 0..20u64 {
        gh.clear_calls();
        let at = now + Duration::from_secs(3 + 120 * i);
        let t = Instant::now();
        w.handle(GhMsg::Refresh(Refresh::Prs), at);
        times.push(t.elapsed());
        let reads = gh.reads();
        assert_eq!(reads.len(), 1, "one gh process for 50 PRs");
        assert_eq!(reads[0].prs.len(), 50);
    }
    times.sort();
    println!(
        "GitHub pass, 50 PRs, one account: median {:?}, max {:?}",
        times[times.len() / 2],
        times[times.len() - 1]
    );
    assert!(times[times.len() - 1] < Duration::from_millis(150));
}
