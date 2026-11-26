//! Trivial fakes of every service, and a clock tests can set. Behind the `fakes` feature:
//!
//! ```toml
//! [dev-dependencies]
//! airboss-services = { path = "../services", features = ["fakes"] }
//! ```
//!
//! Each fake records the calls made to it ([`calls`](FakeSync::calls)), answers from what a
//! test set, and bumps its [`Changes`] when that changes. None of them starts a thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use airboss_core::{RepoId, TaskId};

use crate::{
    AttentionService, Availability, Changes, Clock, Connection, ErrorCode, LaunchReport,
    LaunchService, Mode, NeedsItem, NeedsKey, NeedsList, PrRef, PrView, Prepared, Refresh, Service,
    ServiceError, Source, StartRequest, StoreChange, SyncService, SyncSnapshot, ThreadKey,
    Timestamp,
};

/// The environment id in [`FakeSync`]'s thread URLs.
pub const FAKE_ENVIRONMENT: &str = "fake-environment";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A clock that says what a test set, and moves only when told.
#[derive(Debug)]
pub struct FakeClock(Mutex<SystemTime>);

impl FakeClock {
    /// A clock stopped at `t`.
    pub fn at(t: SystemTime) -> FakeClock {
        FakeClock(Mutex::new(t))
    }

    /// Jumps to `t`, forwards or back.
    pub fn set(&self, t: SystemTime) {
        *lock(&self.0) = t;
    }

    /// Moves forward by `by`, as a sleep would.
    pub fn advance(&self, by: Duration) {
        let mut now = lock(&self.0);
        *now += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *lock(&self.0)
    }
}

/// What every fake shares: its change signal, its connection rows and its call log.
#[derive(Debug, Default)]
struct Base {
    changes: Changes,
    connections: Mutex<Vec<Connection>>,
    calls: Mutex<Vec<String>>,
}

impl Base {
    fn log(&self, call: impl Into<String>) {
        lock(&self.calls).push(call.into());
    }
}

/// Implements [`Service`] for a fake with a `base: Base` field, plus its test helpers.
macro_rules! fake_service {
    ($fake:ty) => {
        impl $fake {
            /// Every call made so far, in order, e.g. `"recheck"` or `"refresh(PrsOf(TaskId(3)))"`.
            pub fn calls(&self) -> Vec<String> {
                lock(&self.base.calls).clone()
            }

            /// What `connections()` returns from now on.
            pub fn set_connections(&self, rows: Vec<Connection>) {
                *lock(&self.base.connections) = rows;
                self.base.changes.bump();
            }

            /// Bumps `changes()` without changing anything.
            pub fn bump(&self) {
                self.base.changes.bump();
            }
        }

        impl Service for $fake {
            fn connections(&self) -> Vec<Connection> {
                lock(&self.base.connections).clone()
            }
            fn recheck(&self) {
                self.base.log("recheck");
            }
            fn changes(&self) -> Changes {
                self.base.changes.clone()
            }
            fn store_changed(&self, c: &StoreChange) {
                self.base
                    .log(format!("store_changed({} events)", c.events.len()));
            }
            fn woke(&self) {
                self.base.log("woke");
            }
            fn shutdown(&self) {
                self.base.log("shutdown");
            }
        }
    };
}

/// A [`SyncService`] that returns the snapshot a test set, and answers `lookup_pr` and
/// `find_thread` from a script.
#[derive(Debug, Default)]
pub struct FakeSync {
    base: Base,
    snapshot: Mutex<Arc<SyncSnapshot>>,
    prs: Mutex<HashMap<PrRef, Result<PrView, ServiceError>>>,
    threads: Mutex<HashMap<(PathBuf, String), ThreadKey>>,
}

fake_service!(FakeSync);

impl FakeSync {
    /// An empty snapshot, no scripted answers.
    pub fn new() -> FakeSync {
        FakeSync::default()
    }

    /// Replaces the snapshot and bumps `changes()`, as a sync pass that changed something does.
    pub fn set_snapshot(&self, snapshot: SyncSnapshot) {
        *lock(&self.snapshot) = Arc::new(snapshot);
        self.base.changes.bump();
    }

    /// What `lookup_pr(pr)` answers. Unscripted refs are `PrNotFound`.
    pub fn answer_lookup(&self, pr: PrRef, answer: Result<PrView, ServiceError>) {
        lock(&self.prs).insert(pr, answer);
    }

    /// What `find_thread(project, branch)` answers. Unscripted pairs are `None`.
    pub fn answer_find_thread(&self, project: impl Into<PathBuf>, branch: &str, key: ThreadKey) {
        lock(&self.threads).insert((project.into(), branch.to_string()), key);
    }
}

impl SyncService for FakeSync {
    fn snapshot(&self) -> Arc<SyncSnapshot> {
        lock(&self.snapshot).clone()
    }

    fn refresh(&self, what: Refresh) {
        self.base.log(format!("refresh({what:?})"));
    }

    fn lookup_pr(&self, pr: &PrRef) -> Result<PrView, ServiceError> {
        self.base.log(format!("lookup_pr({pr})"));
        lock(&self.prs).get(pr).cloned().unwrap_or_else(|| {
            Err(ServiceError::new(
                ErrorCode::PrNotFound,
                format!("{pr} isn't on GitHub."),
            ))
        })
    }

    fn find_thread(&self, project: &Path, branch: &str) -> Option<ThreadKey> {
        self.base
            .log(format!("find_thread({}, {branch})", project.display()));
        lock(&self.threads)
            .get(&(project.to_path_buf(), branch.to_string()))
            .cloned()
    }

    fn thread_url(&self, key: &ThreadKey) -> Option<String> {
        match key.source {
            Source::T3 => Some(format!("t3code://threads/{FAKE_ENVIRONMENT}/{}", key.r#ref)),
            // Claude sessions have no link to open; Herdr terminals open through herdr-app.
            Source::Claude | Source::Herdr => None,
        }
    }
}

/// A [`LaunchService`] that records each `start` and answers with what a test set.
#[derive(Debug)]
pub struct FakeLaunch {
    base: Base,
    availability: Mutex<Availability>,
    herdr_availability: Mutex<Availability>,
    prepare: Mutex<Result<Prepared, ServiceError>>,
    start: Mutex<Result<LaunchReport, ServiceError>>,
    starts: Mutex<Vec<StartRequest>>,
}

fake_service!(FakeLaunch);

impl Default for FakeLaunch {
    fn default() -> FakeLaunch {
        FakeLaunch {
            base: Base::default(),
            availability: Mutex::new(Availability::Ready {
                version: "fake".into(),
            }),
            herdr_availability: Mutex::new(Availability::Missing {
                reason: "Herdr isn't installed.".into(),
            }),
            prepare: Mutex::new(Ok(Prepared {
                repos: Vec::new(),
                from_subtasks: false,
                needs_choice: false,
                choices: Vec::new(),
                branch: None,
                research_dir: PathBuf::from("/tmp/airboss-research"),
                waiting_on: Vec::new(),
                started: Vec::new(),
            })),
            start: Mutex::new(Ok(LaunchReport {
                started: Vec::new(),
                skipped: Vec::new(),
                failed: Vec::new(),
                branch: None,
                moved_to_doing: false,
            })),
            starts: Mutex::new(Vec::new()),
        }
    }
}

impl FakeLaunch {
    /// T3 ready, an empty `Prepared`, and an empty report from `start`.
    pub fn new() -> FakeLaunch {
        FakeLaunch::default()
    }

    /// What `availability()` returns from now on; bumps `changes()`.
    pub fn set_availability(&self, availability: Availability) {
        *lock(&self.availability) = availability;
        self.base.changes.bump();
    }

    /// What `herdr_availability()` returns from now on (Missing at first); bumps `changes()`.
    pub fn set_herdr_availability(&self, availability: Availability) {
        *lock(&self.herdr_availability) = availability;
        self.base.changes.bump();
    }

    /// What every `prepare` answers from now on.
    pub fn set_prepare(&self, answer: Result<Prepared, ServiceError>) {
        *lock(&self.prepare) = answer;
    }

    /// What every `start` answers from now on.
    pub fn set_start(&self, answer: Result<LaunchReport, ServiceError>) {
        *lock(&self.start) = answer;
    }

    /// Every request `start` was given, in order.
    pub fn starts(&self) -> Vec<StartRequest> {
        lock(&self.starts).clone()
    }
}

impl LaunchService for FakeLaunch {
    fn availability(&self) -> Availability {
        lock(&self.availability).clone()
    }

    fn herdr_availability(&self) -> Availability {
        lock(&self.herdr_availability).clone()
    }

    fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError> {
        self.base.log(format!("prepare({task:?}, {mode:?})"));
        lock(&self.prepare).clone()
    }

    fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError> {
        self.base
            .log(format!("start({:?}, {:?})", req.task, req.mode));
        lock(&self.starts).push(req.clone());
        lock(&self.start).clone()
    }

    fn add_to_t3(&self, repo: RepoId) -> Result<(), ServiceError> {
        self.base.log(format!("add_to_t3({repo:?})"));
        Ok(())
    }
}

/// An [`AttentionService`] showing the items a test set. Dismiss and snooze hide an item;
/// restore puts it back where it was. Every visible change bumps `version` and `changes()`.
#[derive(Debug, Default)]
pub struct FakeAttention {
    base: Base,
    list: Mutex<NeedsList>,
    /// Each hidden key, with the item it hid (if it was showing) and where it was.
    hidden: Mutex<HashMap<NeedsKey, Option<(usize, NeedsItem)>>>,
}

fake_service!(FakeAttention);

impl FakeAttention {
    /// An empty list.
    pub fn new() -> FakeAttention {
        FakeAttention::default()
    }

    /// Replaces the visible items, as a recompute would.
    pub fn set_items(&self, items: Vec<NeedsItem>) {
        let mut list = lock(&self.list);
        list.count = u32::try_from(items.len()).unwrap_or(u32::MAX);
        list.items = items;
        list.version += 1;
        self.base.changes.bump();
    }

    /// Sets `checked_at`, as a sync pass would.
    pub fn set_checked_at(&self, at: Option<Timestamp>) {
        lock(&self.list).checked_at = at;
    }

    fn hide(&self, k: &NeedsKey) {
        let mut list = lock(&self.list);
        let found = list.items.iter().position(|i| i.key == *k);
        let entry = found.map(|at| (at, list.items.remove(at)));
        if entry.is_some() {
            list.count = list.count.saturating_sub(1);
            list.version += 1;
            self.base.changes.bump();
        }
        lock(&self.hidden).insert(k.clone(), entry);
    }
}

impl AttentionService for FakeAttention {
    fn needs_me(&self) -> Arc<NeedsList> {
        Arc::new(lock(&self.list).clone())
    }

    fn for_task(&self, task: TaskId) -> Vec<NeedsItem> {
        lock(&self.list)
            .items
            .iter()
            .filter(|i| i.task == task)
            .cloned()
            .collect()
    }

    fn dismiss(&self, k: &NeedsKey) -> Result<(), ServiceError> {
        self.base.log(format!("dismiss({})", k.0));
        self.hide(k);
        Ok(())
    }

    fn snooze(&self, k: &NeedsKey, until: Option<Timestamp>) -> Result<(), ServiceError> {
        self.base.log(format!("snooze({}, {until:?})", k.0));
        self.hide(k);
        Ok(())
    }

    fn restore(&self, k: &NeedsKey) -> Result<(), ServiceError> {
        self.base.log(format!("restore({})", k.0));
        let entry = lock(&self.hidden).remove(k).ok_or_else(|| {
            ServiceError::new(ErrorCode::NotFound, "That item isn't hidden any more.")
        })?;
        if let Some((at, item)) = entry {
            let mut list = lock(&self.list);
            let at = at.min(list.items.len());
            list.items.insert(at, item);
            list.count = list.count.saturating_add(1);
            list.version += 1;
            self.base.changes.bump();
        }
        Ok(())
    }
}
