//! Every T3 check and query against T3's real database, read-only. Run it by hand after
//! each T3 update:
//!
//! ```sh
//! cargo test -p airboss-sync --release --test real_t3 -- --ignored --nocapture
//! ```
//!
//! It opens `~/.t3/userdata/statev2.sqlite` the way `sync` always does: `mode=ro`,
//! `SQLITE_OPEN_READ_ONLY`, `query_only`. It fails if anything there is `Broken`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use airboss_services::time::to_ts;
use airboss_sync::t3::{T3Db, T3Query, T3Source};
use airboss_sync::threads;

fn t3_dir() -> PathBuf {
    Path::new(&std::env::var("HOME").unwrap()).join(".t3/userdata")
}

#[test]
#[ignore = "reads this Mac's real T3 database (read-only); run by hand"]
fn every_query_runs_read_only_on_the_real_database() {
    let mut db = T3Db::new(&t3_dir());
    let opened = Instant::now();
    db.open().expect("the column and value checks pass");
    println!("open + checks (Q0): {:?}", opened.elapsed());
    let env = db.environment_id().unwrap();
    println!(
        "environment id: {}",
        if env.is_some() { "a UUID" } else { "missing" }
    );
    assert!(env.is_some());
    db.data_version().unwrap();

    // The 50 most recent top-level threads, from Q6 over the last 30 days.
    let since = to_ts(SystemTime::now() - Duration::from_secs(30 * 24 * 3600));
    let first = db
        .read(&T3Query {
            untracked_since: Some(since.clone()),
            ..T3Query::default()
        })
        .expect("Q6 and Q8");
    let recent: Vec<String> = first
        .untracked
        .unwrap()
        .into_iter()
        .take(50)
        .map(|t| t.id)
        .collect();
    let projects = first.projects.unwrap();
    println!("projects (Q8): {}", projects.len());
    println!("recent threads: {}", recent.len());
    assert!(!recent.is_empty());

    let q = T3Query {
        linked: recent.clone(),
        untracked_since: Some(since),
        ..T3Query::default()
    };
    let mut times = Vec::new();
    let mut read = None;
    for _ in 0..20 {
        let t = Instant::now();
        read = Some(db.read(&q).expect("Q1-Q6, Q8 and Q4 pass every check"));
        times.push(t.elapsed());
    }
    times.sort();
    let read = read.unwrap();
    println!(
        "one pass, {} linked: median {:?}, max {:?}",
        recent.len(),
        times[times.len() / 2],
        times[times.len() - 1]
    );
    println!(
        "Q1 {} threads, Q2 {} runs, Q3 {} pending requests, Q4 {} failures, Q5 {} roots with children",
        read.threads.len(),
        read.runs.len(),
        read.requests.len(),
        read.failures.len(),
        read.children.len()
    );
    let views = threads::linked(&read, &recent, &HashMap::new());
    let mut states: BTreeMap<String, usize> = BTreeMap::new();
    for v in &views.views {
        *states.entry(format!("{:?}", v.state)).or_default() += 1;
    }
    println!(
        "states: {states:?}; requests waiting for their item: {}",
        views.itemless.len()
    );

    // Q7 for a thread that has a branch and a project.
    if let Some(t) = read
        .threads
        .iter()
        .find(|t| t.branch.is_some() && t.project_path.is_some())
    {
        let found = db
            .find_thread(
                Path::new(t.project_path.as_deref().unwrap()),
                t.branch.as_deref().unwrap(),
            )
            .expect("Q7");
        println!("Q7 found a thread: {}", found.is_some());
        assert!(found.is_some());
    }
}
