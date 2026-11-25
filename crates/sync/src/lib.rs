//! `airboss-sync`: live state from GitHub, T3 and Claude Code, inside the app process.
//!
//! See `SPEC-sync.md` at the repo root. Two worker threads (T3 and Claude; GitHub) and the
//! kqueue watcher's thread; no tokio, no daemon.

pub mod claude;
mod db;
mod finish;
pub mod gh;
mod health;
mod hub;
mod prs;
mod secret;
pub mod t3;
pub mod threads;
pub mod watch;
mod worker;

pub use db::{MIGRATIONS, MODULE, SyncDb};
pub use hub::Hub;
pub use secret::Secret;
pub use worker::{
    Env, GhMsg, GhWorker, MAX_WAIT, Step, StoreSummary, T3Msg, T3Worker, WalWatch, WatchTarget,
};

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use airboss_services::{
    Changes, Clock, Connection, ErrorCode, PrRef, PrView, Refresh, Service, ServiceError,
    SharedStore, Source, StoreChange, SyncService, SyncSnapshot, ThreadKey,
};

use crate::claude::ClaudeSource;
use crate::gh::{GhClient, GhOff, GhOutcome, GhRunner, PrResult};
use crate::t3::T3Source;
use crate::worker::log;

/// Where `sync` reads from, and how often.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConfig {
    /// `~/.t3/userdata`: `statev2.sqlite`, its `-wal`, `environment-id`.
    pub t3_dir: PathBuf,
    /// `~/.claude/sessions`.
    pub claude_sessions: PathBuf,
    /// `None`: look in `/opt/homebrew/bin`, `/usr/local/bin`, then `PATH`.
    pub gh_path: Option<PathBuf>,
    /// The GitHub timer: 5 minutes.
    pub pr_every: Duration,
    /// The least time between two T3 passes: 2 s.
    pub t3_min_gap: Duration,
    /// How far back untracked threads go: 7 days.
    pub untracked_window: Duration,
}

impl SyncConfig {
    /// The standard places under `home`, and the spec's timings.
    pub fn defaults(home: &Path) -> SyncConfig {
        SyncConfig {
            t3_dir: home.join(".t3").join("userdata"),
            claude_sessions: home.join(".claude").join("sessions"),
            gh_path: None,
            pr_every: Duration::from_secs(5 * 60),
            t3_min_gap: Duration::from_secs(2),
            untracked_window: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

#[cfg(feature = "fakes")]
pub mod fake;

/// Everything outside the process `sync` uses, so tests can hand it fakes.
pub struct Tools {
    pub gh: Box<dyn GhRunner>,
    pub t3: Box<dyn T3Source>,
    pub claude: Box<dyn ClaudeSource>,
    pub watch: Box<dyn WalWatch>,
    pub clock: Arc<dyn Clock>,
}

impl Tools {
    /// The real `gh`, T3's database (read-only), the Claude sessions folder with `ps`, the
    /// kqueue watcher and the wall clock. Never called by tests.
    pub fn real(cfg: &SyncConfig) -> Tools {
        Tools {
            gh: Box::new(gh::ProcessGh::new(cfg.gh_path.clone())),
            t3: Box::new(t3::T3Db::new(&cfg.t3_dir)),
            claude: Box::new(claude::RealClaude::new(&cfg.claude_sessions)),
            watch: Box::new(watch::KqueueWatch::new()),
            clock: Arc::new(airboss_services::SystemClock),
        }
    }
}

/// How long `find_thread` waits for the T3 worker.
pub const FIND_THREAD_WAIT: Duration = Duration::from_secs(2);
/// How long `shutdown` waits for the workers.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// The running service: two worker threads, and the hub they publish to. Implements
/// [`SyncService`].
pub struct Sync {
    hub: Arc<Hub>,
    t3: Sender<T3Msg>,
    gh: Sender<GhMsg>,
    client: Arc<Mutex<GhClient>>,
    threads: Mutex<Vec<(JoinHandle<()>, Receiver<()>)>>,
    stopped: std::sync::atomic::AtomicBool,
}

/// What `start` and `run_once` share: both workers, built and not yet started.
struct Built {
    hub: Arc<Hub>,
    t3: T3Worker,
    gh: GhWorker,
    t3_tx: Sender<T3Msg>,
    t3_rx: Receiver<T3Msg>,
    gh_tx: Sender<GhMsg>,
    gh_rx: Receiver<GhMsg>,
    client: Arc<Mutex<GhClient>>,
    clock: Arc<dyn Clock>,
}

fn build(cfg: SyncConfig, store: SharedStore, tools: Tools) -> Result<Built, ServiceError> {
    // The one `core` read at start: which tasks are open (and recently done).
    let snapshot = store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .snapshot()
        .map_err(|e| {
            ServiceError::new(ErrorCode::Core, format!("sync couldn't read tasks: {e}"))
        })?;
    let summary = StoreSummary::from_change(&StoreChange {
        events: Vec::new(),
        snapshot: Arc::new(snapshot),
    });
    let hub = Hub::new();
    let env = Env::new(cfg, store, hub.clone());
    let (t3_tx, t3_rx) = channel();
    let (gh_tx, gh_rx) = channel();
    let client = Arc::new(Mutex::new(GhClient::new(tools.gh)));
    let t3 = T3Worker::new(
        env.clone(),
        tools.t3,
        tools.claude,
        tools.watch,
        t3_tx.clone(),
        gh_tx.clone(),
        summary.open.clone(),
    )?;
    let gh = GhWorker::new(
        env,
        client.clone(),
        t3_tx.clone(),
        summary,
        tools.clock.now(),
    )?;
    Ok(Built {
        hub,
        t3,
        gh,
        t3_tx,
        t3_rx,
        gh_tx,
        gh_rx,
        client,
        clock: tools.clock,
    })
}

/// Starts `sync`: its tables are migrated, the two worker threads start, and the first T3
/// and Claude pass runs at once (GitHub's 3 s later). The app calls this once, with its
/// shared store; `tools` is [`Tools::real`] in the app and fakes in tests.
pub fn start(cfg: SyncConfig, store: SharedStore, tools: Tools) -> Result<Arc<Sync>, ServiceError> {
    let b = build(cfg, store, tools)?;
    let mut threads = Vec::new();
    {
        let (clock, hub, rx) = (b.clock.clone(), b.hub.clone(), b.t3_rx);
        let mut w = b.t3;
        threads.push(spawn("airboss-sync-t3", move |done| {
            run_loop(
                &mut w,
                rx,
                clock,
                hub,
                Machine {
                    start: |w: &mut T3Worker, now| w.start(now),
                    handle: |w: &mut T3Worker, m, now| w.handle(m, now),
                    panicked: |w: &mut T3Worker, now| w.panicked(now),
                    is_shutdown: |m| matches!(m, T3Msg::Shutdown),
                    tick: || T3Msg::Tick,
                },
            );
            let _ = done.send(());
        })?);
    }
    {
        let (clock, hub, rx) = (b.clock.clone(), b.hub.clone(), b.gh_rx);
        let mut w = b.gh;
        threads.push(spawn("airboss-sync-gh", move |done| {
            run_loop(
                &mut w,
                rx,
                clock,
                hub,
                Machine {
                    start: |w: &mut GhWorker, now| w.start(now),
                    handle: |w: &mut GhWorker, m, now| w.handle(m, now),
                    panicked: |w: &mut GhWorker, now| w.panicked(now),
                    is_shutdown: |m| matches!(m, GhMsg::Shutdown),
                    tick: || GhMsg::Tick,
                },
            );
            let _ = done.send(());
        })?);
    }
    Ok(Arc::new(Sync {
        hub: b.hub,
        t3: b.t3_tx,
        gh: b.gh_tx,
        client: b.client,
        threads: Mutex::new(threads),
        stopped: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// One T3 and Claude pass and one GitHub pass, on this thread, for the CLI's `--once`.
pub fn run_once(
    cfg: SyncConfig,
    store: SharedStore,
    tools: Tools,
) -> Result<SyncSnapshot, ServiceError> {
    let mut b = build(cfg, store, tools)?;
    let now = b.clock.now();
    b.t3.start(now);
    b.gh.start(now);
    b.gh.handle(GhMsg::Recheck, now);
    Ok((*b.hub.snapshot()).clone())
}

fn spawn(
    name: &str,
    body: impl FnOnce(Sender<()>) + Send + 'static,
) -> Result<(JoinHandle<()>, Receiver<()>), ServiceError> {
    let (done, exited) = channel();
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || body(done))
        .map(|h| (h, exited))
        .map_err(|e| {
            ServiceError::new(
                ErrorCode::Internal,
                format!("sync couldn't start a thread: {e}"),
            )
        })
}

/// A worker as the thread loop sees it.
struct Machine<W, M> {
    start: fn(&mut W, SystemTime) -> Step,
    handle: fn(&mut W, M, SystemTime) -> Step,
    panicked: fn(&mut W, SystemTime),
    is_shutdown: fn(&M) -> bool,
    tick: fn() -> M,
}

/// Waits for a message or the next deadline (at most 15 minutes), and hands it to the
/// worker. A panic inside the worker is caught, logged and turned into a broken source; the
/// thread keeps running.
fn run_loop<W, M>(
    w: &mut W,
    rx: Receiver<M>,
    clock: Arc<dyn Clock>,
    hub: Arc<Hub>,
    m: Machine<W, M>,
) {
    let guarded = |w: &mut W, f: &dyn Fn(&mut W, SystemTime) -> Step| {
        let now = clock.now();
        match catch_unwind(AssertUnwindSafe(|| f(w, now))) {
            Ok(step) => step,
            Err(_) => {
                log("a sync worker panicked; its source is marked broken");
                let _ = catch_unwind(AssertUnwindSafe(|| (m.panicked)(w, now)));
                Step::at(false, None, now)
            }
        }
    };
    let mut step = guarded(w, &|w, now| (m.start)(w, now));
    loop {
        let wait = step
            .next_wake
            .duration_since(clock.now())
            .unwrap_or(Duration::ZERO)
            .min(MAX_WAIT);
        let msg = match rx.recv_timeout(wait) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) => (m.tick)(),
            Err(RecvTimeoutError::Disconnected) => break,
        };
        hub.woke_up();
        if (m.is_shutdown)(&msg) {
            break;
        }
        let cell = std::cell::Cell::new(Some(msg));
        step = guarded(w, &|w, now| match cell.take() {
            Some(msg) => (m.handle)(w, msg, now),
            None => Step::at(false, None, now),
        });
    }
}

impl Sync {
    /// How many times the worker threads have woken, for the idle measurement.
    pub fn wakeups(&self) -> u64 {
        self.hub.wakeups()
    }

    fn running(&self) -> bool {
        !self.stopped.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn both(&self, t3: T3Msg, gh: GhMsg) {
        if self.running() {
            let _ = self.t3.send(t3);
            let _ = self.gh.send(gh);
        }
    }
}

impl Service for Sync {
    fn connections(&self) -> Vec<Connection> {
        self.hub.connections()
    }

    fn recheck(&self) {
        self.both(T3Msg::Recheck, GhMsg::Recheck);
    }

    fn changes(&self) -> Changes {
        self.hub.changes()
    }

    fn store_changed(&self, c: &StoreChange) {
        let s = StoreSummary::from_change(c);
        self.both(T3Msg::Store(s.clone()), GhMsg::Store(s));
    }

    fn woke(&self) {
        self.both(T3Msg::Woke, GhMsg::Woke);
    }

    fn shutdown(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self.t3.send(T3Msg::Shutdown);
        let _ = self.gh.send(GhMsg::Shutdown);
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        let threads =
            std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        for (handle, exited) in threads {
            let left = deadline.saturating_duration_since(Instant::now());
            if exited.recv_timeout(left).is_ok() || handle.is_finished() {
                let _ = handle.join();
            } else {
                // Still inside a `gh` call (at most 60 s); it exits when that returns.
                log("a sync worker didn't stop within 2 s; leaving it to finish");
            }
        }
    }
}

impl SyncService for Sync {
    fn snapshot(&self) -> Arc<SyncSnapshot> {
        self.hub.snapshot()
    }

    fn refresh(&self, what: Refresh) {
        if !self.running() {
            return;
        }
        match what {
            Refresh::T3 => {
                let _ = self.t3.send(T3Msg::Refresh);
            }
            Refresh::Prs | Refresh::PrsOf(_) => {
                let _ = self.gh.send(GhMsg::Refresh(what));
            }
            Refresh::All => {
                let _ = self.t3.send(T3Msg::Refresh);
                let _ = self.gh.send(GhMsg::Refresh(what));
            }
        }
    }

    fn lookup_pr(&self, pr: &PrRef) -> Result<PrView, ServiceError> {
        if !self.running() {
            return Err(ServiceError::new(
                ErrorCode::ShuttingDown,
                "Airboss is shutting down.",
            ));
        }
        let mut client = worker::gh::lock_client(&self.client);
        let now = SystemTime::now();
        if client.accounts().is_empty() {
            client.read_accounts(now).map_err(|off| match off {
                GhOff::Missing => ServiceError::new(
                    ErrorCode::GhMissing,
                    "Install the GitHub CLI to look up pull requests.",
                ),
                GhOff::SignedOut => ServiceError::new(
                    ErrorCode::SignedOut,
                    "Run `gh auth login` in a terminal to look up pull requests.",
                ),
                GhOff::Failed(why) => ServiceError::new(ErrorCode::GhFailed, why),
            })?;
        }
        let logins: Vec<String> = client
            .accounts()
            .iter()
            .filter(|a| a.ok)
            .map(|a| a.login.clone())
            .collect();
        if logins.is_empty() {
            return Err(ServiceError::new(
                ErrorCode::SignedOut,
                "Run `gh auth login` in a terminal.",
            ));
        }
        let mut last = ServiceError::new(
            ErrorCode::NoAccess,
            format!("None of your GitHub accounts can see {pr}."),
        );
        for login in logins {
            match client.read(&login, std::slice::from_ref(pr), false) {
                GhOutcome::Data { results, .. } => match results.into_iter().next().map(|(_, r)| r)
                {
                    Some(PrResult::Found(d)) => {
                        let linked = self.hub.snapshot().pr_tasks.contains_key(pr);
                        let row = prs::row_from(&d, None, &login, linked, now);
                        let mut view = prs::view(&row, false).ok_or_else(|| {
                            ServiceError::new(
                                ErrorCode::Internal,
                                "GitHub sent a PR sync can't name.",
                            )
                        })?;
                        view.ready_since = None;
                        return Ok(view);
                    }
                    Some(PrResult::NoAccess) | None => continue,
                    Some(PrResult::NotFound) => {
                        return Err(ServiceError::new(
                            ErrorCode::PrNotFound,
                            format!("{pr} isn't on GitHub."),
                        ));
                    }
                    Some(PrResult::Failed(why)) => {
                        return Err(ServiceError::new(ErrorCode::GhFailed, why));
                    }
                },
                GhOutcome::SignedOut => {
                    client.forget_token(&login);
                    last = ServiceError::new(
                        ErrorCode::SignedOut,
                        format!("{login} needs `gh auth login` again."),
                    );
                }
                GhOutcome::SecondaryLimit => {
                    last = ServiceError::new(
                        ErrorCode::RateLimited,
                        "GitHub is rate-limiting requests; try again in a few minutes.",
                    );
                }
                GhOutcome::Failed(why) => {
                    last = ServiceError::new(ErrorCode::GhFailed, why);
                }
            }
        }
        Err(last)
    }

    fn find_thread(&self, project: &Path, branch: &str) -> Option<ThreadKey> {
        if !self.running() {
            return None;
        }
        let (reply, answer) = channel();
        self.t3
            .send(T3Msg::FindThread {
                project: project.to_path_buf(),
                branch: branch.to_string(),
                reply,
            })
            .ok()?;
        let id = answer.recv_timeout(FIND_THREAD_WAIT).ok()??;
        Some(ThreadKey {
            source: Source::T3,
            r#ref: id,
        })
    }

    fn thread_url(&self, key: &ThreadKey) -> Option<String> {
        threads::thread_url(self.hub.environment().as_deref(), key)
    }
}
