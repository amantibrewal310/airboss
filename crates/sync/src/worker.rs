//! The two workers' messages, steps and what they share.
//!
//! Each worker is a plain state machine: `handle(msg, now) -> Step`. The thread loop around
//! it (in `lib.rs`) only waits and forwards, so tests drive the machines directly with
//! chosen times.

pub mod t3;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use airboss_core::{Event, EventKind, LinkKind, Status, Store, TaskId};
use airboss_services::{PrRef, Refresh, ServiceError, SharedStore, StoreChange};

use crate::SyncConfig;
use crate::hub::Hub;

pub use t3::T3Worker;

/// The longest any worker waits before looking at the clock again.
pub const MAX_WAIT: Duration = Duration::from_secs(15 * 60);

/// What one `handle` call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// The snapshot changed.
    pub changed: bool,
    /// When the worker wants to be woken if nothing arrives first; always after `now`, and
    /// at most [`MAX_WAIT`] after it.
    pub next_wake: SystemTime,
}

impl Step {
    /// `next_wake` clamped to `(now, now + MAX_WAIT]`.
    pub(crate) fn at(changed: bool, wanted: Option<SystemTime>, now: SystemTime) -> Step {
        let cap = now + MAX_WAIT;
        let soonest = now + Duration::from_millis(1);
        let next_wake = wanted.map_or(cap, |w| w.clamp(soonest, cap));
        Step { changed, next_wake }
    }
}

/// The part of a [`StoreChange`] a worker needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreSummary {
    /// Tasks that are `todo`, `doing`, `in_review` or `blocked`.
    pub open: HashSet<TaskId>,
    /// The `linked`, `unlinked` and `link_state` events.
    pub link_events: Vec<Event>,
}

/// An open status: `todo`, `doing`, `in_review` or `blocked`.
pub fn is_open(status: Status) -> bool {
    matches!(
        status,
        Status::Todo | Status::Doing | Status::InReview | Status::Blocked
    )
}

impl StoreSummary {
    /// From what the app hands every service after a commit. No `core` read.
    pub fn from_change(c: &StoreChange) -> StoreSummary {
        StoreSummary {
            open: c
                .snapshot
                .tree
                .iter()
                .filter(|n| is_open(n.task.status))
                .map(|n| n.task.id)
                .collect(),
            link_events: c
                .events
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        EventKind::Linked | EventKind::Unlinked | EventKind::LinkState
                    )
                })
                .cloned()
                .collect(),
        }
    }

    /// The link kinds the events name: `core` writes `linked` and `unlinked` bodies as
    /// `<kind> <ref>`, and `link_state` is only ever about a PR.
    pub fn kinds(&self) -> HashSet<LinkKind> {
        self.link_events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::LinkState => Some(LinkKind::Pr),
                _ => {
                    let first = e.body.as_deref()?.split_whitespace().next()?;
                    [
                        LinkKind::T3Thread,
                        LinkKind::ClaudeSession,
                        LinkKind::Pr,
                        LinkKind::Slack,
                        LinkKind::Doc,
                        LinkKind::GithubIssue,
                    ]
                    .into_iter()
                    .find(|k| k.as_str() == first)
                }
            })
            .collect()
    }
}

/// The T3 worker's messages.
#[derive(Debug)]
pub enum T3Msg {
    /// T3 wrote to its `-wal` file.
    Wal,
    /// An entry in T3's folder was added, removed or renamed (re-arms the WAL watch).
    T3Dir,
    /// An entry in the Claude sessions folder changed.
    ClaudeDir,
    /// A Claude session file changed.
    ClaudeFile(PathBuf),
    Store(StoreSummary),
    /// `refresh(T3)` or `refresh(All)`: a pass now, skipping the gap once.
    Refresh,
    /// Q7, answered on `reply`.
    FindThread {
        project: PathBuf,
        branch: String,
        reply: Sender<Option<String>>,
    },
    /// The Mac woke.
    Woke,
    /// "Check again".
    Recheck,
    /// A timeout, or the GitHub worker's timer.
    Tick,
    Shutdown,
}

/// The GitHub worker's messages.
#[derive(Debug)]
pub enum GhMsg {
    Store(StoreSummary),
    Refresh(Refresh),
    /// PRs a T3 thread lists, to read 5 s from now.
    Read(Vec<PrRef>),
    /// The Mac woke.
    Woke,
    /// "Check again".
    Recheck,
    /// A timeout.
    Tick,
    Shutdown,
}

/// What the T3 worker watches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WatchTarget {
    /// T3's folder, non-recursively.
    T3Dir(PathBuf),
    /// T3's `statev2.sqlite-wal`; may not exist while T3 is closed.
    T3Wal(PathBuf),
    /// `~/.claude/sessions`.
    ClaudeDir(PathBuf),
    /// One `<pid>.json` in it.
    ClaudeFile(PathBuf),
}

/// Watches files and sends what happened to the T3 worker. Calling it again with the same
/// targets re-adds any watch the system dropped (a WAL deleted when T3 closed); targets that
/// don't exist yet are skipped until a later call.
pub trait WalWatch: Send {
    fn watch(&mut self, targets: &[WatchTarget], tx: Sender<T3Msg>) -> Result<(), ServiceError>;
}

/// What both workers share.
#[derive(Clone)]
pub struct Env {
    pub cfg: Arc<SyncConfig>,
    pub store: SharedStore,
    pub hub: Arc<Hub>,
}

impl Env {
    pub fn new(cfg: SyncConfig, store: SharedStore, hub: Arc<Hub>) -> Env {
        Env {
            cfg: Arc::new(cfg),
            store,
            hub,
        }
    }

    /// The store, for one short call.
    pub(crate) fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Logs one line to stderr, where the app collects its services' output.
pub(crate) fn log(what: &str) {
    eprintln!("airboss-sync: {what}");
}
