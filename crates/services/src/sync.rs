//! What `sync` shows: threads, PRs, and the trait the app and the other services call.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use airboss_core::{PrState, TaskId};
use serde::{Deserialize, Serialize};

use crate::{PrRef, Service, ServiceError, Timestamp, time};

/// Live state from GitHub, T3 and Claude Code.
pub trait SyncService: Service {
    /// The current snapshot; no I/O.
    fn snapshot(&self) -> Arc<SyncSnapshot>;
    /// Queue a read now instead of at the next tick; returns at once.
    fn refresh(&self, what: Refresh);
    /// Read one PR from GitHub, blocking, for the "Link PR" dialog.
    fn lookup_pr(&self, pr: &PrRef) -> Result<PrView, ServiceError>;
    /// The T3 thread on `branch` in the project holding `project`, if T3 has one.
    fn find_thread(&self, project: &Path, branch: &str) -> Option<ThreadKey>;
    /// `t3code://threads/<environment>/<thread>`, or `None` for a Claude session or when
    /// T3's environment is unknown. No I/O.
    fn thread_url(&self, key: &ThreadKey) -> Option<String>;
}

/// What [`SyncService::refresh`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refresh {
    All,
    Prs,
    /// One task's PRs, merged and closed ones included.
    PrsOf(TaskId),
    T3,
}

/// Everything `sync` knows, at one moment. Kept in memory and handed out as an `Arc`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncSnapshot {
    /// Linked and untracked threads.
    pub threads: HashMap<ThreadKey, ThreadView>,
    /// Linked PRs, and my open PRs.
    pub prs: HashMap<PrRef, PrView>,
    /// The open tasks each linked thread belongs to.
    pub thread_tasks: HashMap<ThreadKey, Vec<TaskId>>,
    /// The open tasks each linked PR belongs to.
    pub pr_tasks: HashMap<PrRef, Vec<TaskId>>,
    /// T3's projects, for first run's "Add repos".
    pub projects: Vec<T3Project>,
    /// The oldest last good read among sources that are ok; `None` if none is.
    pub checked_at: Option<Timestamp>,
}

impl SyncSnapshot {
    /// The threads linked to `task`, newest activity first, ties by key.
    pub fn threads_for(&self, task: TaskId) -> Vec<&ThreadView> {
        let mut out: Vec<&ThreadView> = self
            .thread_tasks
            .iter()
            .filter(|(_, tasks)| tasks.contains(&task))
            .filter_map(|(key, _)| self.threads.get(key))
            .collect();
        newest_first(&mut out, |t| &t.last_activity_at, |t| &t.key);
        out
    }

    /// The PRs linked to `task`, by owner, repo and number.
    pub fn prs_for(&self, task: TaskId) -> Vec<&PrView> {
        let mut out: Vec<&PrView> = self
            .pr_tasks
            .iter()
            .filter(|(_, tasks)| tasks.contains(&task))
            .filter_map(|(pr, _)| self.prs.get(pr))
            .collect();
        out.sort_by(|a, b| a.r#ref.parts().cmp(&b.r#ref.parts()));
        out
    }

    /// Threads linked to no task, newest activity first, ties by key.
    pub fn untracked(&self) -> Vec<&ThreadView> {
        let mut out: Vec<&ThreadView> = self.threads.values().filter(|t| !t.linked).collect();
        newest_first(&mut out, |t| &t.last_activity_at, |t| &t.key);
        out
    }

    /// My open PRs that no task links to, most recently updated first, ties by ref. An
    /// unlinked PR in `prs` can only be mine: `sync` reads no one else's.
    pub fn my_open_prs(&self) -> Vec<&PrView> {
        let mut out: Vec<&PrView> = self
            .prs
            .values()
            .filter(|p| !p.linked && p.state == Some(PrState::Open))
            .collect();
        newest_first(&mut out, |p| &p.updated_at, |p| &p.r#ref);
        out
    }

    /// My open PRs from `branch`, to suggest linking, in [`my_open_prs`](Self::my_open_prs)'s order.
    pub fn suggested_prs(&self, branch: &str) -> Vec<&PrView> {
        self.my_open_prs()
            .into_iter()
            .filter(|p| p.head_ref.as_deref() == Some(branch))
            .collect()
    }
}

/// Sorts by time, newest first and unknown times last, then by `tie`. Times are compared as
/// times, so a GitHub stamp without milliseconds still sorts right.
fn newest_first<T, K: Ord>(
    items: &mut [&T],
    at: impl Fn(&T) -> &Option<Timestamp>,
    tie: impl Fn(&T) -> &K,
) {
    let when = |t: &T| -> Option<SystemTime> { at(t).as_deref().and_then(time::from_ts) };
    items.sort_by(|a, b| {
        Reverse(when(a))
            .cmp(&Reverse(when(b)))
            .then_with(|| tie(a).cmp(tie(b)))
    });
}

/// One of T3's projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct T3Project {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

/// Names one thread: a T3 thread id or a Claude Code session id, the same text as the
/// link's `ref`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ThreadKey {
    pub source: Source,
    pub r#ref: String,
}

/// Where a thread runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    T3,
    Claude,
    /// A terminal in Herdr, by its address.
    Herdr,
}

/// One thread as `sync` last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadView {
    pub key: ThreadKey,
    /// Linked to at least one task; `false` for an untracked thread.
    pub linked: bool,
    pub state: ThreadState,
    pub title: Option<String>,
    /// What it's waiting on me for, when `state` is `NeedsMe`.
    pub request: Option<Request>,
    /// Why its latest run failed, when `state` is `Errored`.
    pub error: Option<String>,
    pub run_id: Option<String>,
    pub project_path: Option<String>,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub parent_ref: Option<String>,
    pub child_count: u32,
    pub last_activity_at: Option<Timestamp>,
    /// The agent's newest top-level reply, where it usually says what's left for me (T3 only).
    /// Not kept in `sync`'s own database: it arrives with the first read after a start.
    #[serde(default)]
    pub last_reply: Option<String>,
    /// Its source is broken or off, so this is the last known state.
    pub stale: bool,
}

/// Where a thread stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadState {
    Running,
    NeedsMe,
    Errored,
    Idle,
    Settled,
    Gone,
}

impl ThreadState {
    /// The live chip the app shows: Running is Working, NeedsMe is WaitingOnYou, Idle is
    /// YourTurn (the agent finished its turn and the next message is mine); Errored and Settled
    /// are Stopped; a Gone thread has no chip.
    pub fn chip(self) -> Option<Chip> {
        match self {
            ThreadState::Running => Some(Chip::Working),
            ThreadState::NeedsMe => Some(Chip::WaitingOnYou),
            ThreadState::Idle => Some(Chip::YourTurn),
            ThreadState::Errored | ThreadState::Settled => Some(Chip::Stopped),
            ThreadState::Gone => None,
        }
    }
}

/// A thread's live chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chip {
    Working,
    WaitingOnYou,
    YourTurn,
    Stopped,
}

/// A question or approval an agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub kind: RequestKind,
    pub text: String,
    pub at: Timestamp,
}

/// What kind of answer a [`Request`] wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Question,
    Approval,
}

/// One PR as GitHub last reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrView {
    pub r#ref: PrRef,
    /// Linked to at least one task; `false` for one of my open PRs.
    pub linked: bool,
    pub url: Option<String>,
    pub title: Option<String>,
    /// `None` until GitHub has been read.
    pub state: Option<PrState>,
    pub draft: bool,
    pub review: Review,
    pub checks: Checks,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub head_ref: Option<String>,
    pub author: Option<String>,
    pub updated_at: Option<Timestamp>,
    /// When it was first seen open and not a draft.
    pub ready_since: Option<Timestamp>,
    /// Why it can't be read, if it can't.
    pub problem: Option<PrProblem>,
    /// Its account is broken or off, so this is the last known state.
    pub stale: bool,
}

/// GitHub's review decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Approved,
    ChangesRequested,
    Required,
    None,
}

/// The latest commit's checks, rolled up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Checks {
    Passing,
    Failing,
    Pending,
    None,
}

/// Why a PR can't be read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrProblem {
    NotFound,
    NoAccess,
    /// `gh` failed; its first line of stderr.
    Failed(String),
}
