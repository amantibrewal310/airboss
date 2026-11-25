//! `airboss-launch`: Do it and Plan it.
//!
//! The only module that runs the `t3` CLI. It commits a `pending` row to its own table before
//! every `t3` call, so a retry, a timeout or a restart never starts a second thread for the
//! same task, repo and mode. See `SPEC-launch.md` at the repo root, and the decisions in
//! `tasks/launch-plan.md`.
//!
//! The app calls [`start`] once, then uses the returned [`Launch`] through
//! `airboss_services::LaunchService`.

// The start loop and the worker use the rest; removed once they land.
#![allow(dead_code)]

pub mod branch;
pub mod brief;
mod core_access;
mod db;
mod errors;
#[cfg(feature = "fakes")]
pub mod fake;
mod git;
mod prepare;
mod process;
mod settle;
mod start;
pub mod t3cli;
mod worker;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use airboss_core::{RepoId, TaskId};
use airboss_services::{
    Availability, Changes, Clock, Connection, ErrorCode, LaunchReport, LaunchService, Mode,
    Prepared, Service, ServiceError, StartRequest, StoreChange, SyncService, SystemClock,
};

pub use core_access::{CoreAccess, CoreWrite, FailWith};
pub use db::MIGRATIONS;
pub use errors::T3_MISSING;
pub use git::{Git, RealGit};
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
        }
    }
}

/// The outside programs and the clock launch uses. Tests pass fakes.
pub struct LaunchTools {
    pub t3: Box<dyn T3Cli>,
    pub git: Box<dyn Git>,
    pub clock: Arc<dyn Clock>,
}

impl LaunchTools {
    /// The real `t3` (from `cfg.t3_path`), `/usr/bin/git` and the wall clock. Never called by
    /// tests: it would run the real `t3`.
    pub fn real(cfg: &LaunchConfig) -> LaunchTools {
        LaunchTools {
            t3: Box::new(ProcessRunner::new(
                cfg.t3_path.clone(),
                cfg.home.clone(),
                child_env(std::env::vars_os()),
            )),
            git: Box::new(RealGit::new(&cfg.home)),
            clock: Arc::new(SystemClock),
        }
    }
}

/// One line on stderr. Launch has no logging crate (SPEC-launch allows no new crate).
pub(crate) fn log(message: &str) {
    eprintln!("airboss-launch: {message}");
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
    pub git: Mutex<GitCache>,
    pub clock: Arc<dyn Clock>,
    pub sync: Arc<dyn SyncService>,
    pub availability: Mutex<Availability>,
    pub changes: Changes,
    pub queue: Sender<LaunchMsg>,
    pub shut: AtomicBool,
    /// Places a `start` in this process is running `t3` for right now. Their pending rows
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
        git: Mutex::new(GitCache {
            git: tools.git,
            defaults: HashMap::new(),
        }),
        clock: tools.clock,
        sync,
        availability: Mutex::new(Availability::Unknown),
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
        self.inner
            .shut
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.inner.queue(LaunchMsg::Shutdown);
    }
}

impl LaunchService for Launch {
    fn availability(&self) -> Availability {
        lock(&self.inner.availability).clone()
    }

    fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError> {
        prepare::prepare(&self.inner, task, mode)
    }

    fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError> {
        start::start(&self.inner, req)
    }

    fn add_to_t3(&self, _repo: RepoId) -> Result<(), ServiceError> {
        Err(ServiceError::new(
            ErrorCode::Internal,
            "Add to T3 isn't built yet.",
        ))
    }
}
