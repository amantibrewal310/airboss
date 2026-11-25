//! Shared test setup: an `AppState` on an in-memory store, with fake services that check the
//! store lock is free whenever they're called, and a recording shell.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, TryLockError};
use std::time::{Duration, SystemTime};

use airboss_app::dto::Patch;
use airboss_app::{AppConfig, AppState, Caller, RecordingShell, Services, UiEvent, share};
use airboss_core::{Actor, NewTask, Store, TaskId};
use airboss_services::fakes::{FakeAttention, FakeClock, FakeLaunch, FakeSync};
use airboss_services::{
    AttentionService, Availability, Changes, Connection, LaunchReport, LaunchService, Mode,
    NeedsItem, NeedsKey, NeedsList, PrRef, PrView, Prepared, Refresh, Service, ServiceError,
    SharedStore, StartRequest, StoreChange, SyncService, SyncSnapshot, ThreadKey, Timestamp, time,
};

/// The fixed "now" every test clock starts at.
pub const NOW: &str = "2026-11-25T10:44:00.000Z";

/// Holds `launch.start` until opened, to test the in-flight guard.
#[derive(Default)]
pub struct Gate {
    closed: Mutex<bool>,
    opened: Condvar,
    pub entered: AtomicUsize,
}

impl Gate {
    pub fn close(&self) {
        *self.closed.lock().unwrap() = true;
    }
    pub fn open(&self) {
        *self.closed.lock().unwrap() = false;
        self.opened.notify_all();
    }
    fn pass(&self) {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.opened.wait(closed).unwrap();
        }
    }
}

/// A fake service that counts calls made while the store lock was held.
pub struct Checked<T> {
    pub inner: Arc<T>,
    store: SharedStore,
    pub violations: Arc<AtomicUsize>,
    gate: Arc<Gate>,
}

impl<T> Checked<T> {
    fn check(&self) {
        if let Err(TryLockError::WouldBlock) = self.store.try_lock() {
            self.violations.fetch_add(1, Ordering::SeqCst);
        }
    }
}

macro_rules! service_impl {
    ($t:ty) => {
        impl Service for Checked<$t> {
            fn connections(&self) -> Vec<Connection> {
                self.check();
                self.inner.connections()
            }
            fn recheck(&self) {
                self.check();
                self.inner.recheck()
            }
            fn changes(&self) -> Changes {
                self.inner.changes()
            }
            fn store_changed(&self, c: &StoreChange) {
                self.check();
                self.inner.store_changed(c)
            }
            fn woke(&self) {
                self.check();
                self.inner.woke()
            }
            fn shutdown(&self) {
                self.check();
                self.inner.shutdown()
            }
        }
    };
}
service_impl!(FakeSync);
service_impl!(FakeLaunch);
service_impl!(FakeAttention);

impl SyncService for Checked<FakeSync> {
    fn snapshot(&self) -> Arc<SyncSnapshot> {
        self.check();
        self.inner.snapshot()
    }
    fn refresh(&self, what: Refresh) {
        self.check();
        self.inner.refresh(what)
    }
    fn lookup_pr(&self, pr: &PrRef) -> Result<PrView, ServiceError> {
        self.check();
        self.inner.lookup_pr(pr)
    }
    fn find_thread(&self, project: &Path, branch: &str) -> Option<ThreadKey> {
        self.check();
        self.inner.find_thread(project, branch)
    }
    fn thread_url(&self, key: &ThreadKey) -> Option<String> {
        self.check();
        self.inner.thread_url(key)
    }
}

impl LaunchService for Checked<FakeLaunch> {
    fn availability(&self) -> Availability {
        self.check();
        self.inner.availability()
    }
    fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError> {
        self.check();
        self.inner.prepare(task, mode)
    }
    fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError> {
        self.check();
        self.gate.pass();
        self.inner.start(req)
    }
    fn add_to_t3(&self, repo: airboss_core::RepoId) -> Result<(), ServiceError> {
        self.check();
        self.inner.add_to_t3(repo)
    }
}

impl AttentionService for Checked<FakeAttention> {
    fn needs_me(&self) -> Arc<NeedsList> {
        self.check();
        self.inner.needs_me()
    }
    fn for_task(&self, task: TaskId) -> Vec<NeedsItem> {
        self.check();
        self.inner.for_task(task)
    }
    fn dismiss(&self, k: &NeedsKey) -> Result<(), ServiceError> {
        self.check();
        self.inner.dismiss(k)
    }
    fn snooze(&self, k: &NeedsKey, until: Option<Timestamp>) -> Result<(), ServiceError> {
        self.check();
        self.inner.snooze(k, until)
    }
    fn restore(&self, k: &NeedsKey) -> Result<(), ServiceError> {
        self.check();
        self.inner.restore(k)
    }
}

/// An app on a fresh store, with every fake reachable.
pub struct TestApp {
    pub state: Arc<AppState>,
    pub store: SharedStore,
    pub shell: Arc<RecordingShell>,
    pub sync: Arc<FakeSync>,
    pub launch: Arc<FakeLaunch>,
    pub attention: Arc<FakeAttention>,
    pub clock: Arc<FakeClock>,
    pub violations: Arc<AtomicUsize>,
    pub home: PathBuf,
    pub gate: Arc<Gate>,
}

/// A unique temporary folder for one test.
pub fn temp_dir(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "airboss-app-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

impl TestApp {
    /// An in-memory store.
    pub fn new() -> TestApp {
        TestApp::with_store(Store::open_in_memory().unwrap())
    }

    /// Any store, e.g. a temporary file.
    pub fn with_store(store: Store) -> TestApp {
        let store = share(store);
        let violations = Arc::new(AtomicUsize::new(0));
        let sync = Arc::new(FakeSync::new());
        let launch = Arc::new(FakeLaunch::new());
        let attention = Arc::new(FakeAttention::new());
        let checked = |v: &Arc<AtomicUsize>| v.clone();
        let gate = Arc::new(Gate::default());
        let services = Services {
            sync: Arc::new(Checked {
                inner: sync.clone(),
                store: store.clone(),
                violations: checked(&violations),
                gate: gate.clone(),
            }),
            launch: Arc::new(Checked {
                inner: launch.clone(),
                store: store.clone(),
                violations: checked(&violations),
                gate: gate.clone(),
            }),
            attention: Arc::new(Checked {
                inner: attention.clone(),
                store: store.clone(),
                violations: checked(&violations),
                gate: gate.clone(),
            }),
        };
        let shell = Arc::new(RecordingShell::new());
        let clock = Arc::new(FakeClock::at(time::from_ts(NOW).unwrap()));
        let home = temp_dir("home");
        let mut config = AppConfig::new(Default::default(), None);
        config.home = home.clone();
        config.reject_delay = Duration::from_millis(150);
        config.fold_save_delay = Duration::from_millis(30);
        let state = AppState::new(
            store.clone(),
            services,
            shell.clone(),
            clock.clone(),
            config,
        )
        .unwrap();
        TestApp {
            state,
            store,
            shell,
            sync,
            launch,
            attention,
            clock,
            violations,
            home,
            gate,
        }
    }

    /// Adds a task as me, straight to `core` (no catch-up).
    pub fn add(&self, title: &str, parent: Option<TaskId>) -> TaskId {
        let mut new = NewTask::new(title);
        new.parent = parent;
        self.store
            .lock()
            .unwrap()
            .add_task(new, &Actor::Me)
            .unwrap()
            .id
    }

    /// Runs `f` on the store directly, as another module would.
    pub fn core<T>(&self, f: impl FnOnce(&mut Store) -> T) -> T {
        f(&mut self.store.lock().unwrap())
    }

    /// Catches up after direct `core` writes.
    pub fn sync_vm(&self) -> Option<Patch> {
        self.state.catch_up().unwrap()
    }

    /// Runs a command by name, as the UI would.
    pub fn call(
        &self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, airboss_app::errors::AppError> {
        airboss_app::ipc::dispatch(&self.state, Caller::Main, command, args)
    }

    /// Every `tree:patch` sent so far.
    pub fn patches(&self) -> Vec<Patch> {
        self.shell
            .events()
            .into_iter()
            .filter_map(|e| match e {
                UiEvent::TreePatch(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    /// No service was called with the store lock held.
    pub fn assert_lock_free(&self) {
        assert_eq!(
            self.violations.load(Ordering::SeqCst),
            0,
            "a service was called with the store lock held"
        );
    }

    /// A timestamp `secs` before the fixed now.
    pub fn ago(&self, secs: u64) -> String {
        time::to_ts(time::from_ts(NOW).unwrap() - Duration::from_secs(secs))
    }
}

/// A Needs me item for tests.
pub fn needs_item(
    key: &str,
    kind: airboss_services::NeedsKind,
    task: TaskId,
    since: &str,
) -> NeedsItem {
    NeedsItem {
        key: NeedsKey(key.into()),
        kind,
        group: kind.group(),
        task,
        task_title: format!("Task {}", task.0),
        since: since.into(),
        headline: "headline".into(),
        context: "context".into(),
        target: airboss_services::Target::Task(task),
        proposed: vec![],
        stale: false,
    }
}

/// `SystemTime` for a timestamp.
pub fn at(ts: &str) -> SystemTime {
    time::from_ts(ts).unwrap()
}
