//! T3: the only code that knows T3's database layout lives here and in `db.rs`.
//!
//! [`T3Source`] hands out typed rows; the state rules are in `threads.rs`.

mod db;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use airboss_services::{T3Project, Timestamp};

pub use db::T3Db;

/// Why T3 couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum T3Error {
    /// No `statev2.sqlite`: T3 isn't installed (T3 sync is off).
    #[error("T3 isn't installed")]
    Missing,
    /// The layout or a value isn't what `sync` knows. It never guesses past this.
    #[error("{0}")]
    Broken(String),
    /// `SQLITE_BUSY` after 200 ms; the next event retries.
    #[error("T3's database is busy")]
    Busy,
}

/// The T3 database, read-only.
pub trait T3Source: Send {
    /// Opens the database and checks its layout and values. Called again hourly, and after
    /// any error, to run the checks again.
    fn open(&mut self) -> Result<(), T3Error>;
    /// `PRAGMA data_version`: changes whenever T3 commits.
    fn data_version(&mut self) -> Result<i64, T3Error>;
    /// Q1–Q6 and Q8 in one read transaction, plus Q4 for new failed runs.
    fn read(&mut self, q: &T3Query) -> Result<T3Read, T3Error>;
    /// Q7: the newest live thread on `branch` in the T3 project holding `project`.
    fn find_thread(&mut self, project: &Path, branch: &str) -> Result<Option<String>, T3Error>;
    /// `environment-id`, trimmed, when it's a UUID.
    fn environment_id(&mut self) -> Result<Option<String>, T3Error>;
}

/// What one pass reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct T3Query {
    /// The threads linked to open tasks (Q1, Q2, Q5).
    pub linked: Vec<String>,
    /// Failed runs whose message is already known, so Q4 isn't run for them again.
    pub known_failures: HashSet<String>,
    /// Q6's cut-off (now minus the untracked window), and with it Q8; `None` skips both.
    pub untracked_since: Option<Timestamp>,
}

/// Everything one pass read, already checked against T3's value lists and required keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct T3Read {
    /// Q1: the linked threads that exist.
    pub threads: Vec<T3Thread>,
    /// Q2: the latest run of each linked thread that has one.
    pub runs: Vec<T3Run>,
    /// Q3: every pending request, oldest first.
    pub requests: Vec<T3Request>,
    /// Q4: a failed run's message, by run id; `None` when it has no error item.
    pub failures: HashMap<String, Option<String>>,
    /// Q5: child threads (subagents, forks) per linked root.
    pub children: HashMap<String, u32>,
    /// Q6, when asked for.
    pub untracked: Option<Vec<T3Untracked>>,
    /// Q8, with Q6.
    pub projects: Option<Vec<T3Project>>,
}

/// One linked thread (Q1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Thread {
    pub id: String,
    pub title: String,
    pub updated_at: Timestamp,
    /// Archived or deleted.
    pub gone: bool,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub parent: Option<String>,
    pub settled_at: Option<Timestamp>,
    pub project_path: Option<String>,
    /// `$.pullRequests` (absent means none).
    pub pull_requests: Vec<T3Pr>,
    /// The text of its newest completed top-level `assistant_message`, trimmed.
    pub last_reply: Option<String>,
}

/// One entry of a thread's `$.pullRequests`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Pr {
    pub host: Option<String>,
    pub repository: Option<String>,
    pub number: Option<u64>,
    pub url: Option<String>,
    pub linked_at: Option<Timestamp>,
}

/// A run's status, from T3's own schema (`OrchestrationV2RunStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStatus {
    Preparing,
    Queued,
    Starting,
    Running,
    Waiting,
    Completed,
    Interrupted,
    Failed,
    Cancelled,
    RolledBack,
}

impl RunStatus {
    pub fn parse(text: &str) -> Option<RunStatus> {
        Some(match text {
            "preparing" => RunStatus::Preparing,
            "queued" => RunStatus::Queued,
            "starting" => RunStatus::Starting,
            "running" => RunStatus::Running,
            "waiting" => RunStatus::Waiting,
            "completed" => RunStatus::Completed,
            "interrupted" => RunStatus::Interrupted,
            "failed" => RunStatus::Failed,
            "cancelled" => RunStatus::Cancelled,
            "rolled_back" => RunStatus::RolledBack,
            _ => return None,
        })
    }

    /// Still going: preparing, queued, starting, running or waiting.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            RunStatus::Preparing
                | RunStatus::Queued
                | RunStatus::Starting
                | RunStatus::Running
                | RunStatus::Waiting
        )
    }
}

/// The latest run of a linked thread (Q2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Run {
    pub thread_id: String,
    pub run_id: String,
    pub status: RunStatus,
    pub requested_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
}

/// A runtime request's kind, from T3's schema (`OrchestrationV2RuntimeRequest.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum T3RequestKind {
    UserInput,
    Command,
    FileRead,
    FileChange,
    McpElicitation,
    Permission,
    DynamicToolCall,
    AuthRefresh,
}

impl T3RequestKind {
    pub fn parse(text: &str) -> Option<T3RequestKind> {
        Some(match text {
            "user_input" => T3RequestKind::UserInput,
            "command" => T3RequestKind::Command,
            "file-read" => T3RequestKind::FileRead,
            "file-change" => T3RequestKind::FileChange,
            "mcp-elicitation" => T3RequestKind::McpElicitation,
            "permission" => T3RequestKind::Permission,
            "dynamic_tool_call" => T3RequestKind::DynamicToolCall,
            "auth_refresh" => T3RequestKind::AuthRefresh,
            _ => return None,
        })
    }
}

/// A request's status, from T3's schema.
pub(crate) const REQUEST_STATUSES: &[&str] = &["pending", "resolved", "expired", "cancelled"];

/// A pending request (Q3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Request {
    pub id: String,
    pub thread_id: String,
    /// The requesting thread's `$.lineage.rootThreadId`.
    pub root: Option<String>,
    pub kind: T3RequestKind,
    pub created_at: Timestamp,
    /// Its turn item, once T3 has written one.
    pub item: Option<T3RequestItem>,
}

/// What a request's turn item says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum T3RequestItem {
    /// A `user_input_request`: `questions[0].question`.
    Question(String),
    /// An `approval_request`: its `prompt`, if it has one.
    Approval(Option<String>),
}

/// A Q6 row: a recent top-level thread, linked or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Untracked {
    pub id: String,
    pub title: String,
    pub updated_at: Timestamp,
    pub settled: bool,
    pub branch: Option<String>,
    pub project_path: Option<String>,
}
