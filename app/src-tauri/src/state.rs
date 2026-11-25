//! `AppState`: the one `core` store, the services, the view model, undo and settings.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime};

use airboss_core::{Store, TaskId};
use airboss_services::{Clock, SharedStore};
use tauri_plugin_log::log;

use crate::catchup::{CatchUp, Trigger};
use crate::dbhealth::DbHealth;
use crate::dto::{ConnectionsChanged, RowDto};
use crate::errors::{AppError, AppResult, Ctx};
use crate::menu::{FocusContext, MenuState, menu_state};
use crate::services::Services;
use crate::settings::{self, Settings};
use crate::shell::Shell;
use crate::undo::{Rejects, UndoStack};
use crate::util::Debounced;
use crate::viewmodel::ViewModel;
use crate::watcher::Watch;

/// Locks a mutex, taking the value even if a holder panicked.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Which window called a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    Main,
    QuickAdd,
    /// Tests, the fixtures recorder, a native menu.
    Other,
}

impl Caller {
    /// From a window label.
    pub fn from_label(label: &str) -> Caller {
        match label {
            "main" => Caller::Main,
            "quickadd" => Caller::QuickAdd,
            _ => Caller::Other,
        }
    }
}

/// How the app is set up; tests shorten the delays.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub settings: Settings,
    /// `None`: settings aren't saved (tests).
    pub settings_path: Option<PathBuf>,
    /// The home folder, for `~/.local/bin` and `~` paths.
    pub home: PathBuf,
    /// How long a reject waits before it's written (5 s).
    pub reject_delay: Duration,
    /// The waits between attempts at a reject the database was too busy to take; each
    /// attempt itself waits out `core`'s 2 s busy timeout first.
    pub reject_retry: Vec<Duration>,
    /// How long folds wait before they're saved (1 s).
    pub fold_save_delay: Duration,
    /// Started by autostart with `--hidden`: don't show the main window.
    pub start_hidden: bool,
    /// When `main` started, for the cold-start log.
    pub started: Instant,
}

impl AppConfig {
    /// The real delays, the home folder from `HOME`, and default settings.
    pub fn new(settings: Settings, settings_path: Option<PathBuf>) -> AppConfig {
        AppConfig {
            settings,
            settings_path,
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/")),
            reject_delay: Duration::from_secs(5),
            reject_retry: vec![Duration::from_millis(250), Duration::from_secs(1)],
            fold_save_delay: Duration::from_secs(1),
            start_hidden: false,
            started: Instant::now(),
        }
    }
}

/// Problems the app itself reports in the connections footer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Problems {
    pub db_moved: bool,
    pub live_stopped: bool,
    pub watcher: Option<String>,
    /// The quick-add hotkey another app holds, as shown ("⌃⌥N").
    pub hotkey_taken: Option<String>,
}

/// Start-up and hotkey timings, for the log and `measure.sh`.
#[derive(Debug, Default)]
pub(crate) struct Timing {
    pub quickadd_pressed: Option<Instant>,
}

/// Everything the commands and app threads share. Built once, behind an `Arc`.
pub struct AppState {
    store: SharedStore,
    pub(crate) services: Services,
    pub(crate) shell: Arc<dyn Shell>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) catchup: Mutex<CatchUp>,
    pub(crate) undo: Mutex<UndoStack>,
    pub(crate) rejects: Rejects,
    pub(crate) launching: Mutex<HashSet<TaskId>>,
    pub(crate) settings: Mutex<Settings>,
    pub(crate) focus: Mutex<FocusContext>,
    pub(crate) triggers: Mutex<Option<Sender<Trigger>>>,
    pub(crate) problems: Mutex<Problems>,
    pub(crate) timing: Mutex<Timing>,
    pub(crate) watch: Mutex<Option<Watch>>,
    /// Whether the database broke while the app runs (R2).
    pub(crate) db: Mutex<DbHealth>,
    pub(crate) config: AppConfig,
    pub(crate) stopping: AtomicBool,
    pub(crate) main_shown: AtomicBool,
    fold_saver: Mutex<Option<Debounced>>,
    poison_logged: AtomicBool,
    me: Weak<AppState>,
}

impl AppState {
    /// Builds the state and its first view model (three small reads).
    pub fn new(
        store: SharedStore,
        services: Services,
        shell: Arc<dyn Shell>,
        clock: Arc<dyn Clock>,
        config: AppConfig,
    ) -> AppResult<Arc<AppState>> {
        let first = CatchUp::first(&store, &services, clock.now())?;
        // After the first reads, so the WAL exists and its identity is the one the app holds.
        let db = DbHealth::new(lock(&store).path());
        let settings = config.settings.clone();
        let state = Arc::new_cyclic(|me| AppState {
            store,
            services,
            shell,
            clock,
            catchup: Mutex::new(first),
            undo: Mutex::new(UndoStack::default()),
            rejects: Rejects::default(),
            launching: Mutex::new(HashSet::new()),
            settings: Mutex::new(settings),
            focus: Mutex::new(FocusContext::default()),
            triggers: Mutex::new(None),
            problems: Mutex::new(Problems::default()),
            timing: Mutex::new(Timing::default()),
            watch: Mutex::new(None),
            db: Mutex::new(db),
            main_shown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            fold_saver: Mutex::new(None),
            poison_logged: AtomicBool::new(false),
            config,
            me: me.clone(),
        });
        {
            let mut cu = lock(&state.catchup);
            cu.connections = state.connections(&cu.vm);
            crate::catchup::log_connections(&cu.connections);
        }
        Ok(state)
    }

    /// Runs one `core` call under the store lock, then drops it. A lock poisoned by a panic is
    /// recovered (rusqlite rolled the open transaction back when it was dropped) and logged once.
    pub fn core<T>(
        &self,
        f: impl FnOnce(&mut Store) -> airboss_core::Result<T>,
    ) -> airboss_core::Result<T> {
        let mut guard = self.store.lock().unwrap_or_else(|poisoned| {
            if !self.poison_logged.swap(true, Ordering::Relaxed) {
                log::error!("the store lock was poisoned by a panic; recovered");
            }
            self.store.clear_poison();
            poisoned.into_inner()
        });
        let result = f(&mut guard);
        drop(guard);
        if let Err(e) = &result {
            // An I/O or corruption error is the database problem state (R2), not a failed save.
            self.saw_core_error(e);
        }
        result
    }

    /// The same, with the error in the user's words.
    pub fn core_as<T>(
        &self,
        ctx: Ctx,
        f: impl FnOnce(&mut Store) -> airboss_core::Result<T>,
    ) -> AppResult<T> {
        self.core(f).map_err(|e| AppError::core(e, ctx))
    }

    /// The shared store, for the services and the watcher.
    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// Reads the view model under the catch-up lock.
    pub fn vm<T>(&self, f: impl FnOnce(&ViewModel) -> T) -> T {
        f(&lock(&self.catchup).vm)
    }

    /// Reads the view model and the event cursor it was built at, under one lock. An undo
    /// entry built from the view model must start at this cursor, not at the newest event:
    /// an agent's write the app hasn't caught up with yet then counts as "changed since".
    pub fn vm_at<T>(&self, f: impl FnOnce(&ViewModel) -> T) -> (T, airboss_core::EventId) {
        let cu = lock(&self.catchup);
        (f(&cu.vm), cu.cursor)
    }

    /// One row as the view model has it.
    pub fn row(&self, id: TaskId) -> Option<RowDto> {
        self.vm(|vm| vm.rows.get(&id).cloned())
    }

    /// The patch sequence number now.
    pub fn seq(&self) -> u64 {
        lock(&self.catchup).seq
    }

    /// The state as an `Arc`, for work handed to another thread.
    pub(crate) fn arc(&self) -> Option<Arc<AppState>> {
        self.me.upgrade()
    }

    /// Runs `f` on its own thread with the state, catching a panic (logged).
    pub(crate) fn spawn(&self, name: &str, f: impl FnOnce(&AppState) + Send + 'static) {
        let Some(me) = self.arc() else { return };
        let name = name.to_string();
        let spawned = std::thread::Builder::new()
            .name(format!("airboss-{name}"))
            .spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&me))).is_err() {
                    log::error!("{name} panicked");
                }
            });
        if let Err(e) = spawned {
            log::error!("couldn't start a thread: {e}");
        }
    }

    /// The time now, from the clock the services share.
    pub fn now(&self) -> SystemTime {
        self.clock.now()
    }

    /// The settings now.
    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// Replaces the settings and saves them now.
    pub(crate) fn store_settings(&self, s: Settings) -> AppResult<()> {
        *lock(&self.settings) = s.clone();
        self.save_settings(&s)
    }

    fn save_settings(&self, s: &Settings) -> AppResult<()> {
        if let Some(path) = &self.config.settings_path {
            settings::save(path, s)
                .map_err(|e| AppError::internal(format!("couldn't save settings: {e}")))?;
        }
        Ok(())
    }

    /// Saves the settings a moment after the last fold changed.
    pub(crate) fn save_folds_soon(&self) {
        let mut saver = lock(&self.fold_saver);
        if saver.is_none() {
            let me = self.me.clone();
            *saver = Debounced::new("folds", self.config.fold_save_delay, move || {
                if let Some(me) = me.upgrade() {
                    let s = me.settings();
                    if let Err(e) = me.save_settings(&s) {
                        log::warn!("{e}");
                    }
                }
            });
        }
        if let Some(saver) = saver.as_ref() {
            saver.poke();
        }
    }

    /// Writes pending folds now (at quit).
    pub(crate) fn flush_folds(&self) {
        if lock(&self.fold_saver).take().is_some() {
            let s = self.settings();
            if let Err(e) = self.save_settings(&s) {
                log::warn!("{e}");
            }
        }
    }

    /// Recomputes the native menus' state from the focus and the undo stack.
    pub(crate) fn refresh_menus(&self) -> MenuState {
        let focus = lock(&self.focus).clone();
        let label = lock(&self.undo).top_label();
        let state = menu_state(&focus, label);
        self.shell.update_menus(&state);
        state
    }

    /// The connection rows and T3's availability, merged as the footer shows them.
    pub(crate) fn connections(&self, vm: &ViewModel) -> ConnectionsChanged {
        let problems = lock(&self.problems).clone();
        let cli = self.config.home.join(".local/bin/airboss");
        let cli_installed = std::fs::symlink_metadata(&cli).is_ok();
        crate::connections::merged(&self.services, vm, &problems, cli_installed)
    }

    /// Re-reads the connections and sends them if they changed.
    pub(crate) fn refresh_connections(&self) {
        let mut cu = lock(&self.catchup);
        let fresh = self.connections(&cu.vm);
        if fresh != cu.connections {
            cu.connections = fresh.clone();
            drop(cu);
            self.shell
                .emit(crate::shell::UiEvent::ConnectionsChanged(fresh));
        }
    }

    /// Adds a problem row (database moved, live updates stopped) and tells the UI.
    pub(crate) fn report_problem(
        &self,
        source: &str,
        message: &str,
        f: impl FnOnce(&mut Problems),
    ) {
        f(&mut lock(&self.problems));
        log::error!("{source}: {message}");
        self.shell.emit(crate::shell::UiEvent::ErrorBackground(
            crate::dto::BackgroundError {
                source: source.to_string(),
                message: message.to_string(),
            },
        ));
        self.refresh_connections();
    }

    /// Registers the saved quick-add hotkey at start-up. If another app holds it, the
    /// connections footer says so (a "Quick add" row) until Settings picks one that works.
    pub fn register_hotkey_at_start(&self) {
        let hotkey = self.settings().hotkey;
        match self.shell.register_hotkey("", &hotkey) {
            Ok(()) => self.set_hotkey_taken(None),
            Err(e) => {
                log::warn!("the hotkey {hotkey} is taken: {e}");
                self.set_hotkey_taken(Some(settings::hotkey_label(&hotkey)));
            }
        }
    }

    /// Shows or clears the "Quick add" row for a hotkey another app holds.
    pub(crate) fn set_hotkey_taken(&self, taken: Option<String>) {
        let changed = {
            let mut p = lock(&self.problems);
            let changed = p.hotkey_taken != taken;
            p.hotkey_taken = taken;
            changed
        };
        if changed {
            self.refresh_connections();
        }
    }

    /// The rows `connections` would show, for tests.
    pub fn connection_rows(&self) -> Vec<airboss_services::Connection> {
        lock(&self.catchup).connections.connections.clone()
    }

    /// Marks a task as starting; the guard clears it when dropped. A second request for the
    /// same task is `AlreadyStarting`.
    pub(crate) fn begin_launch(&self, id: TaskId) -> AppResult<LaunchGuard<'_>> {
        if !lock(&self.launching).insert(id) {
            return Err(AppError::already_starting());
        }
        Ok(LaunchGuard { state: self, id })
    }

    /// Sends a trigger to the catch-up worker, if it's running.
    pub fn trigger(&self, t: Trigger) {
        if let Some(tx) = lock(&self.triggers).as_ref() {
            let _ = tx.send(t);
        }
    }
}

impl AppState {
    /// Quits cleanly: pending rejects are written, folds saved, and each service's
    /// `shutdown()` called on its own thread, waiting at most `limit` in total.
    pub fn shutdown(&self, limit: Duration) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return; // Already shut down (quit raises more than one event).
        }
        self.write_rejects(true);
        self.rejects.wake();
        self.flush_folds();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let services: [Arc<dyn airboss_services::Service>; 3] = [
            self.services.sync.clone(),
            self.services.launch.clone(),
            self.services.attention.clone(),
        ];
        for svc in services {
            let tx = tx.clone();
            let _ = std::thread::Builder::new()
                .name("airboss-shutdown".into())
                .spawn(move || {
                    svc.shutdown();
                    let _ = tx.send(());
                });
        }
        drop(tx);
        let deadline = Instant::now() + limit;
        for _ in 0..3 {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                log::warn!("services didn't stop within {limit:?}");
                break;
            };
            if rx.recv_timeout(left).is_err() {
                log::warn!("services didn't stop within {limit:?}");
                break;
            }
        }
        self.trigger(Trigger::Stop);
        lock(&self.watch).take();
    }
}

/// Holds a task in the in-flight set until `start` returns.
pub(crate) struct LaunchGuard<'a> {
    state: &'a AppState,
    id: TaskId,
}

impl Drop for LaunchGuard<'_> {
    fn drop(&mut self) {
        lock(&self.state.launching).remove(&self.id);
    }
}

/// Wraps a `Store` for sharing: one connection behind one mutex.
pub fn share(store: Store) -> SharedStore {
    Arc::new(Mutex::new(store))
}
