//! The values `core` hands out and takes in.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{EventId, LinkId, RepoId, TaskId};

/// Who is making a change. `core` trusts it; `mcp` proves it.
///
/// Serializes as its stored text form, e.g. `"me"` or `"agent:t3/97b5c8b5"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
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

/// A stored text value `core` doesn't recognise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownValue(pub String);

impl fmt::Display for UnknownValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown value {:?}", self.0)
    }
}

impl std::error::Error for UnknownValue {}

impl FromStr for Actor {
    type Err = UnknownValue;

    /// Reads the stored text form back. An agent needs a non-empty thread ref.
    fn from_str(text: &str) -> Result<Actor, UnknownValue> {
        match text {
            "me" => Ok(Actor::Me),
            "sync:gh" => Ok(Actor::SyncGh),
            "sync:t3" => Ok(Actor::SyncT3),
            "plan" => Ok(Actor::Plan),
            "launch" => Ok(Actor::Launch),
            _ => text
                .strip_prefix("agent:")
                .filter(|r| !r.trim().is_empty())
                .map(|r| Actor::Agent(r.to_string()))
                .ok_or_else(|| UnknownValue(text.to_string())),
        }
    }
}

impl From<Actor> for String {
    fn from(actor: Actor) -> String {
        actor.to_string()
    }
}

impl TryFrom<String> for Actor {
    type Error = UnknownValue;

    fn try_from(text: String) -> Result<Actor, UnknownValue> {
        text.parse()
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

    fn parse(text: &str) -> Option<Status> {
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

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Status {
    type Err = UnknownValue;

    fn from_str(text: &str) -> Result<Status, UnknownValue> {
        Status::parse(text).ok_or_else(|| UnknownValue(text.to_string()))
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

    /// Places the task under `parent`.
    pub fn under(mut self, parent: TaskId) -> NewTask {
        self.parent = Some(parent);
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

/// What `me` changes about a task. `None` leaves a field as it is; for body and repo,
/// `Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEdit {
    pub title: Option<String>,
    pub body: Option<Option<String>>,
    pub repo: Option<Option<RepoId>>,
}

impl TaskEdit {
    /// Renames the task.
    pub fn title(mut self, title: impl Into<String>) -> TaskEdit {
        self.title = Some(title.into());
        self
    }

    /// Replaces the notes; `None` clears them.
    pub fn body(mut self, body: Option<&str>) -> TaskEdit {
        self.body = Some(body.map(str::to_string));
        self
    }

    /// Points the task at a repo; `None` unties it.
    pub fn repo(mut self, repo: Option<RepoId>) -> TaskEdit {
        self.repo = Some(repo);
        self
    }
}

/// What kind of outside thing a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    T3Thread,
    ClaudeSession,
    Pr,
    Slack,
    Doc,
    GithubIssue,
}

impl LinkKind {
    /// The text stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::T3Thread => "t3_thread",
            LinkKind::ClaudeSession => "claude_session",
            LinkKind::Pr => "pr",
            LinkKind::Slack => "slack",
            LinkKind::Doc => "doc",
            LinkKind::GithubIssue => "github_issue",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<LinkKind> {
        Some(match text {
            "t3_thread" => LinkKind::T3Thread,
            "claude_session" => LinkKind::ClaudeSession,
            "pr" => LinkKind::Pr,
            "slack" => LinkKind::Slack,
            "doc" => LinkKind::Doc,
            "github_issue" => LinkKind::GithubIssue,
            _ => return None,
        })
    }
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a linked PR stands; `sync:gh` keeps it current, and rule 6 reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

impl PrState {
    /// The text stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            PrState::Open => "open",
            PrState::Merged => "merged",
            PrState::Closed => "closed",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<PrState> {
        Some(match text {
            "open" => PrState::Open,
            "merged" => PrState::Merged,
            "closed" => PrState::Closed,
            _ => return None,
        })
    }
}

impl fmt::Display for PrState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One outside thing attached to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub id: LinkId,
    pub task: TaskId,
    pub kind: LinkKind,
    /// Thread id, `owner/repo#n`, Slack permalink path or file path.
    pub r#ref: String,
    pub url: Option<String>,
    /// Set exactly for PR links (rule 3).
    pub pr_state: Option<PrState>,
    pub linked_by: Actor,
    pub linked_at: String,
}

/// What a caller supplies to link something. A PR link with no state starts `open`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewLink {
    pub kind: LinkKind,
    pub r#ref: String,
    pub url: Option<String>,
    pub pr_state: Option<PrState>,
}

impl NewLink {
    /// A link of any kind, with no web page.
    pub fn new(kind: LinkKind, r#ref: impl Into<String>) -> NewLink {
        NewLink {
            kind,
            r#ref: r#ref.into(),
            url: None,
            pr_state: None,
        }
    }

    /// A PR link, `owner/repo#n`, starting `open`.
    pub fn pr(r#ref: impl Into<String>) -> NewLink {
        NewLink::new(LinkKind::Pr, r#ref)
    }

    /// Adds the web page it lives at.
    pub fn with_url(mut self, url: impl Into<String>) -> NewLink {
        self.url = Some(url.into());
        self
    }

    /// Records a PR's known state, e.g. one that already merged.
    pub fn with_pr_state(mut self, state: PrState) -> NewLink {
        self.pr_state = Some(state);
        self
    }
}

/// The order a parent's open subtasks can run in (rule 18). Only shown; nothing starts itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOrder {
    /// Wave 1 waits on nothing under the parent, wave 2 only on wave 1, and so on.
    /// Siblings keep their order within a wave.
    pub waves: Vec<Vec<Task>>,
    /// Subtasks that wait, directly or through a sibling, on an open task outside the parent.
    pub waiting_outside: Vec<Task>,
    /// Subtasks that can't be placed in a wave because their lifted dependencies form a
    /// loop, or wait on one, in sibling order. Kept apart so the rest of the order shows.
    pub tangled: Vec<Task>,
}

/// What a plan step waits on: another step in the same plan, or a task that already exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepRef {
    /// The `key` of another step in the same plan.
    Key(String),
    /// A task that already exists, inside the plan's parent or anywhere else.
    Task(TaskId),
}

/// One step of a plan for `add_tasks`, with its own steps under it (rule 19).
///
/// The `key` is the plan's own id for the step. Sending the plan again matches steps by
/// key, not title, so a renamed step isn't duplicated (rule 20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTask {
    pub key: String,
    pub title: String,
    pub body: Option<String>,
    pub repo: Option<RepoId>,
    #[serde(default)]
    pub depends_on: Vec<DepRef>,
    #[serde(default)]
    pub children: Vec<PlanTask>,
}

impl PlanTask {
    /// A step with a key and a title, and nothing under it.
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> PlanTask {
        PlanTask {
            key: key.into(),
            title: title.into(),
            body: None,
            repo: None,
            depends_on: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Adds notes.
    pub fn with_body(mut self, body: impl Into<String>) -> PlanTask {
        self.body = Some(body.into());
        self
    }

    /// Ties the step to a repo.
    pub fn in_repo(mut self, repo: RepoId) -> PlanTask {
        self.repo = Some(repo);
        self
    }

    /// Makes the step wait on another step of the same plan.
    pub fn after(mut self, key: impl Into<String>) -> PlanTask {
        self.depends_on.push(DepRef::Key(key.into()));
        self
    }

    /// Makes the step wait on a task that already exists.
    pub fn after_task(mut self, task: TaskId) -> PlanTask {
        self.depends_on.push(DepRef::Task(task));
        self
    }

    /// Adds a step under this one, after any already added.
    pub fn child(mut self, child: PlanTask) -> PlanTask {
        self.children.push(child);
        self
    }
}

/// What `add_tasks` did with each step, in plan order (depth-first).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanResult {
    /// Every step's key and the task it now is, so a caller can map keys to ids.
    pub keys: Vec<(String, TaskId)>,
    /// Tasks this call created.
    pub created: Vec<TaskId>,
    /// Steps whose key already existed under the same parent, left as they were.
    pub existing: Vec<TaskId>,
}

impl PlanResult {
    /// The task a step's key resolved to.
    pub fn id(&self, key: &str) -> Option<TaskId> {
        self.keys.iter().find(|(k, _)| k == key).map(|&(_, id)| id)
    }
}
