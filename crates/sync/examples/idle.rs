//! The idle measurement (SPEC-sync, "Performance targets"): `sync` running with T3 and
//! GitHub idle, at the real 5-minute timer, for an hour.
//!
//! ```sh
//! cargo build --release -p airboss-sync --example idle --features fakes
//! /usr/bin/time -l target/release/examples/idle 3600
//! ```
//!
//! Everything is real except GitHub: T3's queries run on a fixture database in a temp dir
//! (nothing writes to it during the run), kqueue watches its folder and WAL, the Claude
//! sessions folder is an empty temp dir, and the wall clock drives the timers. `gh` is
//! `FakeGh`, so no `gh` process runs; its cost is measured on its own. At the end it prints
//! how often the worker threads woke, and the process's CPU time comes from `time -l`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use airboss_core::{Actor, LinkKind, NewLink, NewTask, Store};
use airboss_services::{Service, SyncService, SystemClock};
use airboss_sync::claude::RealClaude;
use airboss_sync::fake::{FakeGh, FakeHerdr, FakePr, T3Fixture, TempDir};
use airboss_sync::t3::T3Db;
use airboss_sync::watch::KqueueWatch;
use airboss_sync::{SyncConfig, Tools, start};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let t3 = T3Fixture::with_rows();
    let claude = TempDir::new();
    let core = TempDir::new();
    let store = Store::open(&core.path().join("airboss.db"))?;
    let store = Arc::new(std::sync::Mutex::new(store));
    {
        let mut s = store.lock().map_err(|_| "lock")?;
        for (thread, pr) in [
            ("t-run", "acme/web#1"),
            ("t-settled", "acme/web#2"),
            ("t-idle", "acme/web#3"),
        ] {
            let t = s.add_task(NewTask::new(format!("Work on {thread}")), &Actor::Me)?;
            s.link(t.id, NewLink::new(LinkKind::T3Thread, thread), &Actor::Me)?;
            s.link(t.id, NewLink::pr(pr), &Actor::Me)?;
        }
    }
    let gh = FakeGh::new()
        .account("me", true)
        .sees("me", "acme/web")
        .pr("acme/web#1", FakePr::open("me"))
        .pr("acme/web#2", FakePr::open("me"))
        .pr("acme/web#3", FakePr::open("me"));
    let mut cfg = SyncConfig::defaults(core.path());
    cfg.t3_dir = t3.dir().to_path_buf();
    cfg.claude_sessions = claude.path().to_path_buf();
    let tools = Tools {
        gh: Box::new(gh.clone()),
        t3: Box::new(T3Db::new(t3.dir())),
        claude: Box::new(RealClaude::new(claude.path())),
        herdr: Box::new(FakeHerdr::new()),
        watch: Box::new(KqueueWatch::new()),
        clock: Arc::new(SystemClock),
    };
    let sync = start(cfg, store, tools)?;
    println!("pid {} running idle for {secs} s", std::process::id());
    let began = Instant::now();
    let wakes_at_start = sync.wakeups();
    std::thread::sleep(Duration::from_secs(secs));
    let elapsed = began.elapsed().as_secs_f64();
    let wakes = sync.wakeups() - wakes_at_start;
    let passes = gh.reads().len();
    println!(
        "{elapsed:.0} s: {wakes} worker wake-ups ({:.1} an hour), {passes} GitHub passes ({:.1} an hour), {} threads in the snapshot",
        wakes as f64 * 3600.0 / elapsed,
        passes as f64 * 3600.0 / elapsed,
        sync.snapshot().threads.len()
    );
    sync.shutdown();
    Ok(())
}
