//! `airboss-services`: the contract between the Airboss app and its services.
//!
//! Types and traits only, from the "Shared contract" section of `SPEC-attention.md`. It uses
//! `std` alone (no tokio, no threads) and has no logic beyond [`Changes`], [`PrRef`],
//! [`ThreadState::chip`], [`NeedsKind::group`], the [`SyncSnapshot`] helpers, [`time`] and
//! [`log`], the one line every service logs through.
//!
//! Enums serialise as snake_case strings; [`PrRef`], [`NeedsKey`] and [`Timestamp`] as plain
//! strings. See `README.md` for the choices made where the specs left a gap.

mod attention;
mod changes;
mod launch;
pub mod log;
mod pr_ref;
mod sync;
pub mod time;

#[cfg(feature = "fakes")]
pub mod fakes;

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use airboss_core::{CoreSnapshot, Event};
use serde::{Deserialize, Serialize};

pub use attention::{
    AttentionService, NeedsGroup, NeedsItem, NeedsKey, NeedsKind, NeedsList, Target,
};
pub use changes::Changes;
pub use launch::{
    Availability, HerdrAgent, LaunchReport, LaunchService, Launcher, Mode, Place, Prepared,
    StartRequest, StartedThread,
};
pub use pr_ref::PrRef;
pub use sync::{
    Checks, Chip, PrProblem, PrView, Refresh, Request, RequestKind, Review, Source, SyncService,
    SyncSnapshot, T3Project, ThreadKey, ThreadState, ThreadView,
};

/// The one `core` store every service in the app process shares. One lock per call.
pub type SharedStore = Arc<Mutex<airboss_core::Store>>;

/// UTC text in `core`'s format, `"2026-11-25T17:19:07.841Z"`. Made with [`time::to_ts`], so text
/// order is time order.
pub type Timestamp = String;

/// Where services get the time, so tests can choose it.
pub trait Clock: Send + Sync {
    /// The wall clock, or a test's chosen time.
    fn now(&self) -> SystemTime;
}

/// The real clock: `SystemTime::now()`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// What the app hands every service after it commits to `core`: the new events, and a
/// snapshot read in the same moment, so no service has to read `core` again.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreChange {
    pub events: Vec<Event>,
    pub snapshot: Arc<CoreSnapshot>,
}

/// Every service.
///
/// `store_changed`, `recheck` and `woke` queue work and return at once; `connections` and
/// `changes` read memory only. The app calls them from a background thread with no lock held.
pub trait Service: Send + Sync {
    /// This service's lines in the connections popover.
    fn connections(&self) -> Vec<Connection>;
    /// Check every outside connection again now (the "Check again" button).
    fn recheck(&self);
    /// Bumped whenever something the app shows from this service changed.
    fn changes(&self) -> Changes;
    /// `core` changed; queue whatever follows from it.
    fn store_changed(&self, c: &StoreChange);
    /// The Mac woke: re-read the wall clock and catch up on anything overdue.
    fn woke(&self);
    /// Stop every thread the service runs, and return once they have.
    fn shutdown(&self);
}

/// One line of the connections popover.
///
/// Rows with the same `name` from several services are one row (the app merges them): ok when
/// all are ok, with `state`, `detail` and `fix` from the first that isn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    /// "GitHub", "T3 Code", "Claude Code"; the app adds "Agents" (the airboss command).
    pub name: String,
    pub ok: bool,
    /// Short, e.g. "Signed in" or "Not found".
    pub state: String,
    /// What the connection is for.
    pub detail: String,
    /// What to do about it, when it isn't ok.
    pub fix: Option<String>,
}

/// Why a service call failed, said once for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceError {
    pub code: ErrorCode,
    /// One sentence for the user.
    pub message: String,
}

impl ServiceError {
    /// An error with its code and its one sentence.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ServiceError {
        ServiceError {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ServiceError {}

/// What kind of failure a [`ServiceError`] is. The app acts on `NeedsChoice`, `NotAProject`,
/// `TaskClosed`, `T3Missing` and `T3NotRunning`; the rest only pass through to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // launch
    T3Missing,
    T3NotRunning,
    T3Unreachable,
    T3TooOld,
    Timeout,
    NotAProject,
    RepoMissing,
    NoBaseBranch,
    BadBranch,
    TaskClosed,
    TaskProposed,
    NeedsChoice,
    Waiting,
    AlreadyStarting,
    BadAnswer,
    T3Failed,
    /// Herdr's `hc` or `herdr-app` isn't installed.
    HerdrMissing,
    /// Herdr's window didn't answer, even after opening it.
    HerdrNotRunning,
    /// `git worktree add` failed for a Herdr Do it; the message is git's.
    WorktreeFailed,
    /// `herdr-app` or `hc` answered with an error.
    HerdrFailed,
    // sync
    GhMissing,
    SignedOut,
    NoAccess,
    PrNotFound,
    BadPrRef,
    RateLimited,
    GhFailed,
    T3Unavailable,
    // any service
    NotFound,
    Core,
    Storage,
    Internal,
    ShuttingDown,
}
