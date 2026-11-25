//! The values `core` hands out and takes in.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{EventId, RepoId, TaskId};

/// Who is making a change. `core` trusts it; `mcp` proves it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Actor {
    /// The person using Airboss.
    Me,
    /// An agent thread, named by its thread ref, e.g. `t3/97b5c8b5`.
    Agent(String),
    /// GitHub sync, recording PR state.
    SyncGh,
    /// T3 sync, recording thread activity.
    SyncT3,
    /// A plan I settled, arriving through `add_tasks`.
    Plan,
    /// The launcher that starts threads for tasks.
    Launch,
}

impl Actor {
    /// Reads the stored text form back; `None` for anything unknown.
    pub fn parse(text: &str) -> Option<Actor> {
        match text {
            "me" => Some(Actor::Me),
            "sync:gh" => Some(Actor::SyncGh),
            "sync:t3" => Some(Actor::SyncT3),
            "plan" => Some(Actor::Plan),
            "launch" => Some(Actor::Launch),
            _ => text
                .strip_prefix("agent:")
                .filter(|r| !r.is_empty())
                .map(|r| Actor::Agent(r.to_string())),
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::Me => f.write_str("me"),
            Actor::Agent(r) => write!(f, "agent:{r}"),
            Actor::SyncGh => f.write_str("sync:gh"),
            Actor::SyncT3 => f.write_str("sync:t3"),
            Actor::Plan => f.write_str("plan"),
            Actor::Launch => f.write_str("launch"),
        }
    }
}

/// Where a task stands. See SPEC-core rule 6 for who may move it where.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Proposed,
    Todo,
    Doing,
    InReview,
    Blocked,
    Done,
    Dropped,
}

impl Status {
    /// The text stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Proposed => "proposed",
            Status::Todo => "todo",
            Status::Doing => "doing",
            Status::InReview => "in_review",
            Status::Blocked => "blocked",
            Status::Done => "done",
            Status::Dropped => "dropped",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Status> {
        Some(match text {
            "proposed" => Status::Proposed,
            "todo" => Status::Todo,
            "doing" => Status::Doing,
            "in_review" => Status::InReview,
            "blocked" => Status::Blocked,
            "done" => Status::Done,
            "dropped" => Status::Dropped,
            _ => return None,
        })
    }
}

/// How a task came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Me,
    Agent,
    Plan,
}

impl Origin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Origin::Me => "me",
            Origin::Agent => "agent",
            Origin::Plan => "plan",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Origin> {
        Some(match text {
            "me" => Origin::Me,
            "agent" => Origin::Agent,
            "plan" => Origin::Plan,
            _ => return None,
        })
    }
}

/// One thing to get done, at any depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub parent: Option<TaskId>,
    pub repo: Option<RepoId>,
    pub title: String,
    pub body: Option<String>,
    pub status: Status,
    pub blocked_reason: Option<String>,
    pub position: u32,
    pub origin: Origin,
    pub plan_key: Option<String>,
    pub created_at: String,
}

/// What a caller supplies to add one task.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTask {
    pub title: String,
    pub parent: Option<TaskId>,
    pub repo: Option<RepoId>,
    pub body: Option<String>,
}

impl NewTask {
    /// A top-level task with just a title.
    pub fn new(title: impl Into<String>) -> NewTask {
        NewTask {
            title: title.into(),
            ..NewTask::default()
        }
    }

    /// Places the task under `parent` (`None` keeps it top-level).
    pub fn under(mut self, parent: Option<TaskId>) -> NewTask {
        self.parent = parent;
        self
    }

    /// Ties the task to a repo.
    pub fn in_repo(mut self, repo: RepoId) -> NewTask {
        self.repo = Some(repo);
        self
    }

    /// Adds notes.
    pub fn with_body(mut self, body: impl Into<String>) -> NewTask {
        self.body = Some(body.into());
        self
    }
}

/// A task as it appears in the tree view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskNode {
    pub task: Task,
    /// 0 for a top-level task.
    pub depth: u32,
    /// True while any task it waits on is neither done nor dropped (rule 16).
    pub waiting: bool,
}

/// A git repo tasks can point at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: String,
    pub remote_url: Option<String>,
}

/// What kind of change an event records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Created,
    Edited,
    Moved,
    Status,
    Note,
    Linked,
    Unlinked,
    LinkState,
    WaitsOnAdded,
    WaitsOnRemoved,
}

impl EventKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            EventKind::Created => "created",
            EventKind::Edited => "edited",
            EventKind::Moved => "moved",
            EventKind::Status => "status",
            EventKind::Note => "note",
            EventKind::Linked => "linked",
            EventKind::Unlinked => "unlinked",
            EventKind::LinkState => "link_state",
            EventKind::WaitsOnAdded => "waits_on_added",
            EventKind::WaitsOnRemoved => "waits_on_removed",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<EventKind> {
        Some(match text {
            "created" => EventKind::Created,
            "edited" => EventKind::Edited,
            "moved" => EventKind::Moved,
            "status" => EventKind::Status,
            "note" => EventKind::Note,
            "linked" => EventKind::Linked,
            "unlinked" => EventKind::Unlinked,
            "link_state" => EventKind::LinkState,
            "waits_on_added" => EventKind::WaitsOnAdded,
            "waits_on_removed" => EventKind::WaitsOnRemoved,
            _ => return None,
        })
    }
}

/// One change to one task. Never edited (rule 22).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub task: TaskId,
    pub kind: EventKind,
    pub to_status: Option<Status>,
    pub body: Option<String>,
    pub actor: Actor,
    pub at: String,
}
