//! `airboss-launch`: Do it and Plan it.
//!
//! The only module that runs the `t3` CLI, and Herdr's `herdr-app` and `hc` to start agents
//! in Herdr tabs. It commits a `pending` row to its own table before
//! every start, so a retry, a timeout or a restart never starts a second thread for the
//! same task, repo and mode. See `SPEC-launch.md` at the repo root, and the decisions in
//! `tasks/launch-plan.md`.
//!
//! The app calls [`start`] once, then uses the returned [`Launch`] through
//! `airboss_services::LaunchService`.

pub mod branch;
pub mod brief;
mod core_access;
mod db;
mod errors;
#[cfg(feature = "fakes")]
pub mod fake;
mod git;
pub mod herdr;
mod prepare;
mod process;
mod settle;
mod start;
pub mod t3cli;
mod worker;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use airboss_core::{RepoId, TaskId};
use airboss_services::{
    Availability, Changes, Clock, Connection, ErrorCode, LaunchReport, LaunchService, Mode,
    Prepared, Service, ServiceError, StartRequest, StoreChange, SyncService, SystemClock,
};

pub use core_access::{CoreAccess, CoreWrite, FailWith};
pub use db::MIGRATIONS;
pub use errors::{HERDR_MISSING, T3_MISSING};
pub use git::{Git, RealGit, Worktree};
pub use herdr::{HerdrCli, RealHerdr};
pub use process::{CmdOutput, ProcessRunner, REMOVED_VARS, child_env};
pub use start::Brief;
pub use t3cli::T3Cli;
pub use worker::{LaunchMsg, LaunchWorker, Step};

use crate::db::Db;

/// Where launch finds `t3`, where research threads start, and how long it waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchConfig {
    /// `~/.t3/bin/t3`.
    pub t3_path: PathBuf,
    /// `~/.t3/scratch` (T3's "No project"); returned as `Prepared::research_dir`.
    pub research_dir: PathBuf,
    /// The longest one `t3` call may take: 30 s.
    pub timeout: Duration,
    /// A pending Do it row not found by `find_thread` this long after it was made is `lost`:
    /// 3 minutes.
    pub settle_after: Duration,
    /// The working directory for `t3` and `git`: `$HOME`.
    pub home: PathBuf,
    /// Launch's own tables. `None`: the same file as `core`'s (`Store::path()`).
    pub db_path: Option<PathBuf>,
    /// Herdr's settings, read only for `worktrees.directory`:
    /// `~/.config/herdr-app/settings.json`.
    pub herdr_settings: PathBuf,
    /// A pending Herdr row whose terminal no live terminal's labels match this long after it
    /// was made is `lost`: 2 minutes.
    pub herdr_settle_after: Duration,
}

impl LaunchConfig {
    /// The paths on this Mac, under `home`.
    pub fn defaults(home: &Path) -> LaunchConfig {
        LaunchConfig {
            t3_path: home.join(".t3/bin/t3"),
            research_dir: home.join(".t3/scratch"),
            timeout: Duration::from_secs(30),
            settle_after: Duration::from_secs(180),
            home: home.to_path_buf(),
            db_path: None,
            herdr_settings: home.join(".config/herdr-app/settings.json"),
            herdr_settle_after: Duration::from_secs(120),
        }
    }
}

/// The outside programs and the clock launch uses. Tests pass fakes.
pub struct LaunchTools {
    pub t3: Box<dyn T3Cli>,
    pub git: Box<dyn Git>,
    pub herdr: Box<dyn HerdrCli>,
    pub clock: Arc<dyn Clock>,
}

impl LaunchTools {
    /// The real `t3` (from `cfg.t3_path`), `/usr/bin/git`, Herdr's `herdr-app` and `hc`
    /// (from `~/.local/bin`, else `PATH`) and the wall clock. Never called by tests: it would
    /// run the real `t3` and Herdr.
    pub fn real(cfg: &LaunchConfig) -> LaunchTools {
        LaunchTools {
            t3: Box::new(ProcessRunner::new(
                cfg.t3_path.clone(),
                cfg.home.clone(),
                child_env(std::env::vars_os()),
            )),
            git: Box::new(RealGit::new(&cfg.home)),
            herdr: Box::new(RealHerdr::new(&cfg.home)),
            clock: Arc::new(SystemClock),
        }
    }
}

/// One line through the services' log, which the app sends to its log file. Launch has no
/// logging crate (SPEC-launch allows no new crate).
pub(crate) fn log(message: &str) {
    airboss_services::log::line("airboss-launch", message);
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Each guarded value is left whole by every holder (one SQLite statement or transaction,
    // one process run, one map insert), so a holder's panic can't leave it half-changed.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `git` plus the default branch of each repo, looked up once per run of the app.
pub(crate) struct GitCache {
    git: Box<dyn Git>,
    defaults: HashMap<PathBuf, Option<String>>,
}

impl GitCache {
    /// The repo's default branch, asked of `git` once per run of the app.
    pub fn default_branch(&mut self, repo: &Path) -> std::io::Result<Option<String>> {
        if let Some(known) = self.defaults.get(repo) {
            return Ok(known.clone());
        }
        let found = self.git.default_branch(repo)?;
        self.defaults.insert(repo.to_path_buf(), found.clone());
        Ok(found)
    }
}

/// What `Launch` and its worker share.
pub(crate) struct Inner {
    pub cfg: LaunchConfig,
    pub core: CoreAccess,
    pub db: Mutex<Db>,
    pub t3: Mutex<Box<dyn T3Cli>>,
    pub herdr: Mutex<Box<dyn HerdrCli>>,
    pub git: Mutex<GitCache>,
    pub clock: Arc<dyn Clock>,
    pub sync: Arc<dyn SyncService>,
    pub availability: Mutex<Availability>,
    pub herdr_availability: Mutex<Availability>,
    pub changes: Changes,
    pub queue: Sender<LaunchMsg>,
    pub shut: AtomicBool,
    /// Places a `start` in this process is starting right now. Their pending rows
    /// aren't stale, so "Start again" doesn't replace them.
    pub in_flight: Mutex<HashSet<PlaceKey>>,
}

/// One place: a task, a repo (or none) and a mode.
pub(crate) type PlaceKey = (TaskId, Option<RepoId>, Mode);

impl Inner {
    /// Queues work for the worker. A worker that has stopped has nothing left to do.
    pub fn queue(&self, msg: LaunchMsg) {
        let _ = self.queue.send(msg);
    }
}

/// Do it and Plan it. Implements `airboss_services::LaunchService`.
pub struct Launch {
    inner: Arc<Inner>,
    worker: Mutex<Option<worker::Running>>,
}

/// The longest `shutdown` waits for the worker.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// Starts launch for the app: [`open`], plus the worker thread, which checks T3 and Herdr
/// and settles any pending rows left by the last run straight away.
///
/// `cfg` is usually `LaunchConfig::defaults(home)`; `core` wraps the app's `SharedStore`;
/// `tools` is `LaunchTools::real(&cfg)`.
pub fn start(
    cfg: LaunchConfig,
    core: CoreAccess,
    sync: Arc<dyn SyncService>,
    tools: LaunchTools,
) -> Result<Arc<Launch>, ServiceError> {
    let (launch, worker) = open(cfg, core, sync, tools)?;
    let (done_tx, done) = mpsc::channel();
    let join = std::thread::Builder::new()
        .name("airboss-launch".into())
        .spawn(move || {
            worker.run();
            let _ = done_tx.send(());
        })
        .map_err(|e| {
            ServiceError::new(
                ErrorCode::Internal,
                format!("Couldn't start launch's worker: {e}."),
            )
        })?;
    *lock(&launch.worker) = Some(worker::Running { done, join });
    Ok(launch)
}

/// Exactly what `start` would send for `req`, for golden tests and a preview. Places that
/// already started are left out; a place that would fail makes the whole call fail.
pub fn briefs(launch: &Launch, req: &StartRequest) -> Result<Vec<Brief>, ServiceError> {
    start::briefs(&launch.inner, req)
}

/// Builds `Launch` and its worker without starting a thread, so tests can drive the worker
/// with chosen times. The worker's first jobs (check availability, settle) are queued.
pub fn open(
    cfg: LaunchConfig,
    core: CoreAccess,
    sync: Arc<dyn SyncService>,
    tools: LaunchTools,
) -> Result<(Arc<Launch>, LaunchWorker), ServiceError> {
    let db_path = match (&cfg.db_path, core.path()) {
        (Some(p), _) => p.clone(),
        (None, Some(p)) => p,
        (None, None) => return Err(errors::no_database()),
    };
    let db = Db::open(&db_path)?;
    let (tx, rx) = mpsc::channel();
    let inner = Arc::new(Inner {
        cfg,
        core,
        db: Mutex::new(db),
        t3: Mutex::new(tools.t3),
        herdr: Mutex::new(tools.herdr),
        git: Mutex::new(GitCache {
            git: tools.git,
            defaults: HashMap::new(),
        }),
        clock: tools.clock,
        sync,
        availability: Mutex::new(Availability::Unknown),
        herdr_availability: Mutex::new(Availability::Unknown),
        changes: Changes::new(),
        queue: tx,
        shut: AtomicBool::new(false),
        in_flight: Mutex::new(HashSet::new()),
    });
    inner.queue(LaunchMsg::CheckAvailability);
    inner.queue(LaunchMsg::Settle);
    let launch = Arc::new(Launch {
        inner: inner.clone(),
        worker: Mutex::new(None),
    });
    Ok((launch, LaunchWorker::new(inner, rx)))
}

/// The connections popover's "T3 Code" row for an availability.
fn connection(availability: &Availability) -> Connection {
    let name = "T3 Code".to_string();
    match availability {
        Availability::Ready { .. } => Connection {
            name,
            ok: true,
            state: "Connected".into(),
            detail: "Do it and Plan it start agent threads here.".into(),
            fix: None,
        },
        Availability::Missing { .. } => Connection {
            name,
            ok: false,
            state: "Not found".into(),
            detail: "Do it and Plan it start agent threads here.".into(),
            fix: Some(
                "Install T3 Code to turn on Do it and Plan it. Everything else works.".into(),
            ),
        },
        Availability::Unknown => Connection {
            name,
            ok: true,
            state: "Checking…".into(),
            detail: "Do it and Plan it start agent threads here.".into(),
            fix: None,
        },
    }
}

impl Service for Launch {
    fn connections(&self) -> Vec<Connection> {
        vec![connection(&lock(&self.inner.availability))]
    }

    fn recheck(&self) {
        self.inner.queue(LaunchMsg::CheckAvailability);
        self.inner.queue(LaunchMsg::Settle);
    }

    fn changes(&self) -> Changes {
        self.inner.changes.clone()
    }

    fn store_changed(&self, _c: &StoreChange) {}

    fn woke(&self) {
        self.inner.queue(LaunchMsg::Woke);
    }

    fn shutdown(&self) {
        self.inner.shut.store(true, Ordering::SeqCst);
        self.inner.queue(LaunchMsg::Shutdown);
        let Some(running) = lock(&self.worker).take() else {
            return;
        };
        // The worker finishes what it's doing first; wait at most a second for it.
        match running.done.recv_timeout(SHUTDOWN_WAIT) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if running.join.join().is_err() {
                    log("the worker thread ended with a panic");
                }
            }
            Err(RecvTimeoutError::Timeout) => log("the worker didn't stop within a second"),
        }
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl LaunchService for Launch {
    fn availability(&self) -> Availability {
        lock(&self.inner.availability).clone()
    }

    fn herdr_availability(&self) -> Availability {
        lock(&self.inner.herdr_availability).clone()
    }

    fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError> {
        prepare::prepare(&self.inner, task, mode)
    }

    fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError> {
        start::start(&self.inner, req)
    }

    fn add_to_t3(&self, repo: RepoId) -> Result<(), ServiceError> {
        start::add_to_t3(&self.inner, repo)
    }
}
