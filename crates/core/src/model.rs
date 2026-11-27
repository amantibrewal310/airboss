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

/// How urgent a task is, P0 the most. A task may have none; then it sorts after P3.
///
/// Serializes as `"p0"` to `"p3"` and prints as `P0` to `P3`; it's stored as 0 to 3. It only
/// ranks tasks for filtering and sorting: sibling order within a parent stays its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl Priority {
    /// All four, most urgent first.
    pub const ALL: [Priority; 4] = [Priority::P0, Priority::P1, Priority::P2, Priority::P3];

    /// The number stored in the database, 0 to 3.
    pub(crate) fn as_int(self) -> i64 {
        self as i64
    }

    /// Reads the stored number back.
    pub(crate) fn from_int(n: i64) -> Option<Priority> {
        Priority::ALL.into_iter().find(|p| p.as_int() == n)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.as_int())
    }
}

impl FromStr for Priority {
    type Err = UnknownValue;

    /// Reads `P0` to `P3`, in either case.
    fn from_str(text: &str) -> Result<Priority, UnknownValue> {
        Priority::ALL
            .into_iter()
            .find(|p| p.to_string().eq_ignore_ascii_case(text.trim()))
            .ok_or_else(|| UnknownValue(text.to_string()))
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
    /// The repos it's linked to, sorted by repo name. Only its own, not its subtasks'.
    pub repos: Vec<RepoId>,
    pub title: String,
    pub body: Option<String>,
    /// Its own status, or, when it has steps, what they add up to (rule 10).
    pub status: Status,
    /// Why it's blocked. On a parent, which step is blocked and why (rule 10).
    pub blocked_reason: Option<String>,
    pub position: u32,
    pub origin: Origin,
    pub plan_key: Option<String>,
    /// The branch every thread for this task shares; `None` until one is chosen.
    pub branch: Option<String>,
    /// How urgent it is; `None` when no priority is set. Its own, never worked out from steps.
    pub priority: Option<Priority>,
    pub created_at: String,
}

/// What a caller supplies to add one task.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTask {
    pub title: String,
    pub parent: Option<TaskId>,
    pub repos: Vec<RepoId>,
    pub body: Option<String>,
    #[serde(default)]
    pub priority: Option<Priority>,
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

    /// Links the task to a repo. Call it again for more; a repo given twice is linked once.
    pub fn in_repo(mut self, repo: RepoId) -> NewTask {
        if !self.repos.contains(&repo) {
            self.repos.push(repo);
        }
        self
    }

    /// Adds notes.
    pub fn with_body(mut self, body: impl Into<String>) -> NewTask {
        self.body = Some(body.into());
        self
    }

    /// Gives it a priority.
    pub fn with_priority(mut self, priority: Priority) -> NewTask {
        self.priority = Some(priority);
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
    /// True when it has steps: subtasks that aren't proposals. Its status is then worked out
    /// from them, and it takes no branch, PR or Do it of its own (rule 10).
    pub has_steps: bool,
    /// When its own newest event happened (rule 28).
    pub last_event_at: String,
    /// Who made its own newest event.
    pub last_actor: Actor,
    /// When the newest event in it or anywhere under it happened, for "quiet for 5+ days".
    pub subtree_last_event_at: String,
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
    /// I turned down an agent's proposal. The task is `dropped` and hidden from the tree.
    Rejected,
    /// A repo was linked to the task; the body names it.
    RepoLinked,
    /// A repo was unlinked from the task; the body names it.
    RepoUnlinked,
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
            EventKind::Rejected => "rejected",
            EventKind::RepoLinked => "repo_linked",
            EventKind::RepoUnlinked => "repo_unlinked",
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
            "rejected" => EventKind::Rejected,
            "repo_linked" => EventKind::RepoLinked,
            "repo_unlinked" => EventKind::RepoUnlinked,
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

/// What `me` changes about a task. `None` leaves a field as it is; for the body and the
/// priority, `Some(None)` clears it. Repos change through `link_repo` and `unlink_repo`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEdit {
    pub title: Option<String>,
    pub body: Option<Option<String>>,
    #[serde(default)]
    pub priority: Option<Option<Priority>>,
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

    /// Sets the priority; `None` clears it.
    pub fn priority(mut self, priority: Option<Priority>) -> TaskEdit {
        self.priority = Some(priority);
        self
    }
}

/// What kind of outside thing a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    T3Thread,
    ClaudeSession,
    /// A terminal in Herdr, by its address (`t4p`, `vps/t7`).
    HerdrTerminal,
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
            LinkKind::HerdrTerminal => "herdr_terminal",
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
            "herdr_terminal" => LinkKind::HerdrTerminal,
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
    /// What the app shows, e.g. "#booking thread"; `None` means show the ref.
    pub title: Option<String>,
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
    #[serde(default)]
    pub title: Option<String>,
}

impl NewLink {
    /// A link of any kind, with no web page.
    pub fn new(kind: LinkKind, r#ref: impl Into<String>) -> NewLink {
        NewLink {
            kind,
            r#ref: r#ref.into(),
            url: None,
            pr_state: None,
            title: None,
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

    /// Names the link for display, e.g. "booking-flow.md"; a blank title is no title.
    pub fn with_title(mut self, title: impl Into<String>) -> NewLink {
        self.title = Some(title.into());
        self
    }

    /// Records a PR's known state, e.g. one that already merged.
    pub fn with_pr_state(mut self, state: PrState) -> NewLink {
        self.pr_state = Some(state);
        self
    }
}

/// A task that can be picked up now, and since when (rule 26).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyTask {
    pub task: Task,
    /// When its last blocker finished or was removed; its creation time if it never waited.
    pub since: String,
}

/// Everything the app and the services redraw from, read at one moment (rule 30).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSnapshot {
    /// The same as `tree()`.
    pub tree: Vec<TaskNode>,
    /// Every `(waiting, waits_on)` pair, the same as `dependency_edges()`.
    pub edges: Vec<(TaskId, TaskId)>,
    /// The same as `ready()`, worked out from `tree`.
    pub ready: Vec<ReadyTask>,
    /// The newest event id when the snapshot was read; `EventId(0)` when there are none.
    pub latest_event_id: EventId,
}

/// Every dependency of one task, both ways, finished tasks included (rule 27).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependencies {
    /// What the task waits on, by id. Done and dropped blockers are here too.
    pub waits_on: Vec<Task>,
    /// What waits on the task, by id.
    pub blocks: Vec<Task>,
}

/// What `reject_proposals` did with each id it was given, in the order given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectOutcome {
    /// Still `proposed` when the change was written, so now `dropped` with a `rejected` event.
    pub rejected: Vec<TaskId>,
    /// No longer `proposed` by then (accepted, or already dropped), with the status each has.
    /// Left exactly as they were. A task that no longer exists is in neither list.
    pub skipped: Vec<(TaskId, Status)>,
}

/// The order a parent's open subtasks can run in (rule 18). Only shown; nothing starts itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOrder {
    /// Wave 1 waits on nothing under the parent, wave 2 only on wave 1, and so on.
    /// Siblings keep their order within a wave.
    pub waves: Vec<Vec<Task>>,
    /// Subtasks that wait, directly or through a sibling, on an open task outside the parent.
    pub waiting_outside: Vec<OutsideWait>,
    /// Subtasks that can't be placed in a wave because their lifted dependencies form a
    /// loop, or wait on one, in sibling order. Kept apart so the rest of the order shows.
    pub tangled: Vec<Task>,
}

/// A subtask held back by open tasks outside its parent (rule 18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutsideWait {
    pub task: Task,
    /// The open tasks outside the parent that hold it, directly, through its subtree, or
    /// through a sibling it waits on; sorted by id.
    pub blockers: Vec<TaskId>,
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
    #[serde(default)]
    pub repos: Vec<RepoId>,
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
            repos: Vec::new(),
            depends_on: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Adds notes.
    pub fn with_body(mut self, body: impl Into<String>) -> PlanTask {
        self.body = Some(body.into());
        self
    }

    /// Links the step to a repo. Call it again for more; a repo given twice is linked once.
    pub fn in_repo(mut self, repo: RepoId) -> PlanTask {
        if !self.repos.contains(&repo) {
            self.repos.push(repo);
        }
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
