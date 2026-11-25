//! What `launch` takes and gives: Do it, Plan it, and the trait the app calls.

use std::path::PathBuf;

use airboss_core::{Repo, RepoId, TaskId};
use serde::{Deserialize, Serialize};

use crate::{Service, ServiceError, ThreadKey};

/// The Do it and Plan it buttons.
pub trait LaunchService: Service {
    /// Whether T3 Code is there to start threads in; cached, no process.
    fn availability(&self) -> Availability;
    /// What `start` would do for this task, without doing it. May block briefly.
    fn prepare(&self, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError>;
    /// Starts the threads. Blocks for seconds; `Err` only when nothing could begin.
    fn start(&self, req: &StartRequest) -> Result<LaunchReport, ServiceError>;
    /// Adds a repo to T3 as a project (the "Add to T3 and retry" button).
    fn add_to_t3(&self, repo: RepoId) -> Result<(), ServiceError>;
}

/// Whether T3 Code can start threads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Not checked yet.
    Unknown,
    Ready {
        version: String,
    },
    Missing {
        reason: String,
    },
}

/// Which button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// One thread per linked repo, each in its own worktree.
    DoIt,
    /// One thread that plans the task with me.
    PlanIt,
}

/// Where the threads start.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// In the task's repos (or its subtasks', when it has none of its own).
    TaskRepos,
    /// "No repo: research only", in this folder.
    NoRepo(PathBuf),
}

/// One press of Do it or Plan it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRequest {
    pub task: TaskId,
    pub mode: Mode,
    pub place: Place,
    /// "Start again": start new threads even where some already started or are starting.
    pub again: bool,
}

/// What `start` would do, for the task panel and the "where to start" choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prepared {
    pub repos: Vec<Repo>,
    /// `repos` came from the open subtasks, because the task has none of its own.
    pub from_subtasks: bool,
    /// Do it has no repo to start in: the app asks where.
    pub needs_choice: bool,
    /// Every registered repo, for that question.
    pub choices: Vec<Repo>,
    /// The branch Do it would use, not saved yet.
    pub branch: Option<String>,
    pub research_dir: PathBuf,
    /// Open blockers: Do it refuses while there are any.
    pub waiting_on: Vec<TaskId>,
    /// Threads already started for this task.
    pub started: Vec<StartedThread>,
}

/// What one `start` did, place by place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchReport {
    pub started: Vec<StartedThread>,
    /// Repos that already had a thread for this task and mode.
    pub skipped: Vec<RepoId>,
    /// Places that failed, each with why; `None` for the research folder.
    pub failed: Vec<(Option<RepoId>, ServiceError)>,
    pub branch: Option<String>,
    /// The task moved to `doing` in this call.
    pub moved_to_doing: bool,
}

/// One thread `launch` started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartedThread {
    pub key: ThreadKey,
    pub mode: Mode,
    /// `None` for the research folder.
    pub repo: Option<RepoId>,
    pub title: String,
    /// Linked to its task already; `false` until `launch`'s worker retries a failed link.
    pub linked: bool,
}
