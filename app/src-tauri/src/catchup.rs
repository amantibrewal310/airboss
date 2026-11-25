//! The one rebuild path. Every trigger leads to `catch_up()`, which rebuilds from one
//! `core.snapshot()` and sends a patch only if something changed.
//!
//! Triggers: our own writes (inline, and through `subscribe()`), the WAL watcher (writes from
//! other processes), the main window gaining focus and the Mac waking (`data_version()`), and
//! any service's `Changes` bump. They all go to one channel, read by the catch-up worker, which
//! waits 30 ms after the last trigger and never more than 150 ms after the first.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use airboss_core::{EventId, Repo};
use airboss_services::{SharedStore, StoreChange};
use tauri_plugin_log::log;

use crate::dto::{ConnectionsDto, NeedsDto, Patch};
use crate::errors::{AppError, AppResult};
use crate::services::Services;
use crate::shell::UiEvent;
use crate::state::{AppState, lock};
use crate::viewmodel::{ViewModel, diff};

/// The debounce: wait this long after the last trigger …
pub const QUIET: Duration = Duration::from_millis(30);
/// … but never longer than this after the first.
pub const MAX_WAIT: Duration = Duration::from_millis(150);
/// No service listener waits longer than this in one go.
pub const LISTEN_MAX: Duration = Duration::from_secs(15 * 60);

/// Why the worker should look again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// `core` may have changed: our write, another process, the WAL.
    Core,
    /// The main window gained focus: check `data_version()`.
    Focus,
    /// The Mac woke: tell the services, then check `data_version()`.
    Wake,
    /// A service's `Changes` bumped.
    Service,
    /// Stop the worker (quit, or a test ending).
    Stop,
}

/// What the catch-up lock owns: the view model, the event cursor, `seq` and what was last sent.
pub(crate) struct CatchUp {
    pub vm: ViewModel,
    pub cursor: EventId,
    pub seq: u64,
    pub data_version: Option<i64>,
    pub needs_version: u64,
    pub connections: ConnectionsDto,
}

fn core_read<T>(
    store: &SharedStore,
    f: impl FnOnce(&airboss_core::Store) -> airboss_core::Result<T>,
) -> AppResult<T> {
    let guard = lock(store);
    f(&guard).map_err(|e| AppError::core(e, crate::errors::Ctx::Read))
}

impl CatchUp {
    /// The first view model: one snapshot, the repos and `data_version`, one lock each.
    pub(crate) fn first(
        store: &SharedStore,
        services: &Services,
        now: SystemTime,
    ) -> AppResult<CatchUp> {
        let snapshot = Arc::new(core_read(store, |s| s.snapshot())?);
        let repos = core_read(store, |s| s.repos())?;
        let data_version = core_read(store, |s| s.data_version())?;
        let needs = services.attention.needs_me();
        let vm = ViewModel::build(
            snapshot.clone(),
            repos,
            services.sync.snapshot(),
            needs.clone(),
            now,
        );
        Ok(CatchUp {
            cursor: snapshot.latest_event_id,
            vm,
            seq: 1,
            data_version: Some(data_version),
            needs_version: needs.version,
            connections: ConnectionsDto {
                connections: Vec::new(),
                availability: crate::dto::AvailabilityDto::Unknown,
            },
        })
    }
}

impl AppState {
    /// Catches up with `core`: two small reads when nothing changed; otherwise one snapshot,
    /// the services told (no lock held), a rebuild and a patch.
    pub fn catch_up(&self) -> AppResult<Option<Patch>> {
        let mut cu = lock(&self.catchup);
        let cursor = cu.cursor;
        let mut events = self.core(|s| s.events_after(cursor)).map_err(read_err)?;
        let repos: Vec<Repo> = self.core(|s| s.repos()).map_err(read_err)?;
        if events.is_empty() && repos == cu.vm.repos {
            return Ok(None);
        }
        let snapshot = Arc::new(self.core(|s| s.snapshot()).map_err(read_err)?);
        let last = events.last().map_or(cursor, |e| e.id);
        if snapshot.latest_event_id > last {
            let more = self.core(|s| s.events_after(last)).map_err(read_err)?;
            events.extend(
                more.into_iter()
                    .filter(|e| e.id <= snapshot.latest_event_id),
            );
        }
        cu.cursor = cu.cursor.max(snapshot.latest_event_id);
        // The store lock is free here: each `core` call above dropped it.
        let change = StoreChange {
            events,
            snapshot: snapshot.clone(),
        };
        for s in self.services.all() {
            s.store_changed(&change);
        }
        let patch = self.rebuild(&mut cu, Some((snapshot, repos)));
        drop(cu);
        if patch.is_some() {
            self.prune_undo();
        }
        Ok(patch)
    }

    /// Rebuilds from the last `core` snapshot and the services' new ones (no `core` read).
    pub fn catch_up_services(&self) -> Option<Patch> {
        let mut cu = lock(&self.catchup);
        self.rebuild(&mut cu, None)
    }

    /// On focus and wake: catch up only if another connection committed since we last looked.
    pub fn check_data_version(&self) -> AppResult<Option<Patch>> {
        let dv = self.core(|s| s.data_version()).map_err(read_err)?;
        {
            let mut cu = lock(&self.catchup);
            if cu.data_version == Some(dv) {
                return Ok(None);
            }
            cu.data_version = Some(dv);
        }
        self.catch_up()
    }

    /// After a write: catch up inline and return the patch, or an empty one at the current
    /// `seq` if the worker already sent it (or nothing changed).
    pub(crate) fn after_write(&self) -> AppResult<Patch> {
        match self.catch_up()? {
            Some(p) => Ok(p),
            None => Ok(Patch::empty(self.seq())),
        }
    }

    /// Builds a new view model, diffs it, and sends what changed: `tree:patch`,
    /// `needs:changed` (and the menu-bar item) and `connections:changed`.
    fn rebuild(
        &self,
        cu: &mut CatchUp,
        core: Option<(Arc<airboss_core::CoreSnapshot>, Vec<Repo>)>,
    ) -> Option<Patch> {
        let (snapshot, repos) =
            core.unwrap_or_else(|| (cu.vm.snapshot.clone(), cu.vm.repos.clone()));
        let needs = self.services.attention.needs_me();
        let vm = ViewModel::build(
            snapshot,
            repos,
            self.services.sync.snapshot(),
            needs.clone(),
            self.now(),
        );
        let patch = diff(&cu.vm, &vm).map(|mut p| {
            cu.seq += 1;
            p.seq = cu.seq;
            p
        });
        cu.vm = vm;
        if let Some(p) = &patch {
            self.shell.emit(UiEvent::TreePatch(p.clone()));
        }
        if needs.version != cu.needs_version {
            cu.needs_version = needs.version;
            self.shell.update_tray(&needs);
            self.shell
                .emit(UiEvent::NeedsChanged(NeedsDto::from(&*needs)));
        }
        let connections = self.connections(&cu.vm);
        if connections != cu.connections {
            cu.connections = connections.clone();
            self.shell.emit(UiEvent::ConnectionsChanged(connections));
        }
        patch
    }

    /// Drops undo entries whose tasks no longer exist.
    fn prune_undo(&self) {
        let ids: std::collections::HashSet<_> = self.vm(|vm| vm.rows.keys().copied().collect());
        lock(&self.undo).retain_tasks(&ids);
    }

    /// Starts the app threads: the catch-up worker, the `subscribe()` forwarder, the WAL
    /// watcher, one listener per service and the deferred-reject timer.
    pub fn start_background(self: &Arc<Self>) {
        let (tx, rx) = mpsc::channel::<Trigger>();
        *lock(&self.triggers) = Some(tx.clone());

        let me = self.clone();
        guarded(self, "catch-up", move |restarted| {
            if restarted {
                // A panic may have left a trigger unhandled: look once more.
                me.process(Causes::all());
            }
            worker(&me, &rx);
        });

        let sub = self.core(|s| Ok(s.subscribe()));
        if let Ok(sub) = sub {
            let tx = tx.clone();
            guarded(self, "subscribe", move |_| {
                while sub.recv().is_ok() {
                    if tx.send(Trigger::Core).is_err() {
                        return;
                    }
                }
            });
        }

        if let Some(path) = self.core(|s| Ok(s.path())).ok().flatten() {
            let weak = Arc::downgrade(self);
            match crate::watcher::Watch::start(&path, tx.clone(), move |why| {
                if let Some(me) = weak.upgrade() {
                    me.report_problem("database", why, |p| p.db_moved = true);
                }
            }) {
                Ok(w) => *lock(&self.watch) = Some(w),
                Err(e) => self.report_problem(
                    "watcher",
                    &format!("couldn't watch the database: {e}"),
                    |p| p.watcher = Some("Airboss couldn't watch the database file.".into()),
                ),
            }
        }

        for (name, service) in [
            ("sync", self.services.all()[0].changes()),
            ("launch", self.services.all()[1].changes()),
            ("attention", self.services.all()[2].changes()),
        ] {
            let tx = tx.clone();
            let me = self.clone();
            guarded(self, name, move |_| {
                let mut seen = service.current();
                while !me.stopping.load(Ordering::Relaxed) {
                    let now = service.wait_past(seen, LISTEN_MAX);
                    if now > seen {
                        seen = now;
                        if tx.send(Trigger::Service).is_err() {
                            return;
                        }
                    }
                }
            });
        }

        let me = self.clone();
        guarded(self, "rejects", move |_| crate::undo::reject_timer(&me));
    }

    /// Handles a batch of triggers.
    fn process(&self, causes: Causes) {
        if causes.wake {
            for s in self.services.all() {
                s.woke();
            }
        }
        if (causes.focus || causes.wake)
            && let Err(e) = self.check_data_version()
        {
            log::warn!("catch-up on focus or wake failed: {e}");
        }
        if causes.core
            && let Err(e) = self.catch_up()
        {
            log::warn!("catch-up failed: {e}");
        }
        if causes.service {
            // A core catch-up that found nothing new doesn't rebuild; the services may have
            // changed on their own.
            self.catch_up_services();
        }
    }
}

fn read_err(e: airboss_core::Error) -> AppError {
    AppError::core(e, crate::errors::Ctx::Read)
}

/// Which kinds of trigger arrived in one debounce window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Causes {
    core: bool,
    focus: bool,
    wake: bool,
    service: bool,
}

impl Causes {
    fn all() -> Causes {
        Causes {
            core: true,
            focus: false,
            wake: false,
            service: true,
        }
    }

    fn add(&mut self, t: Trigger) {
        match t {
            Trigger::Core => self.core = true,
            Trigger::Focus => self.focus = true,
            Trigger::Wake => self.wake = true,
            Trigger::Service => self.service = true,
            Trigger::Stop => {}
        }
    }
}

/// Reads triggers until `Stop` or the channel closes, debouncing each batch.
fn worker(state: &AppState, rx: &Receiver<Trigger>) {
    while let Ok(first) = rx.recv() {
        if first == Trigger::Stop {
            return;
        }
        let mut causes = Causes::default();
        causes.add(first);
        let start = Instant::now();
        let mut last = start;
        loop {
            let deadline = (last + QUIET).min(start + MAX_WAIT);
            let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match rx.recv_timeout(wait) {
                Ok(Trigger::Stop) => return,
                Ok(t) => {
                    causes.add(t);
                    last = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    state.process(causes);
                    return;
                }
            }
        }
        state.process(causes);
    }
}

/// Runs an app thread's loop in `catch_unwind`. After one panic it runs again (told it was
/// restarted); after a second it stops, and the footer says live updates stopped.
pub fn guarded(state: &Arc<AppState>, name: &'static str, body: impl FnMut(bool) + Send + 'static) {
    let weak = Arc::downgrade(state);
    let mut body = body;
    let spawned = std::thread::Builder::new()
        .name(format!("airboss-{name}"))
        .spawn(move || {
            let mut panics = 0;
            loop {
                let restarted = panics > 0;
                match catch_unwind(AssertUnwindSafe(|| body(restarted))) {
                    Ok(()) => return,
                    Err(_) => {
                        panics += 1;
                        log::error!("the {name} thread panicked ({panics})");
                        if panics >= 2 {
                            if let Some(me) = weak.upgrade() {
                                me.report_problem(
                                    name,
                                    "Live updates stopped after an internal error. Restart Airboss.",
                                    |p| p.live_stopped = true,
                                );
                            }
                            return;
                        }
                    }
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("couldn't start the {name} thread: {e}");
    }
}
