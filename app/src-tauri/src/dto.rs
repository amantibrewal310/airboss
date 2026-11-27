//! Every type the UI sees, with its TypeScript type from ts-rs.
//!
//! The shapes follow the UI's contract (`app/src/lib/api/contract.ts` and the types it
//! imports). Values from `core` and `crates/services` are sent as they serialise themselves;
//! since those crates don't depend on ts-rs, each has a TypeScript-only mirror below (the
//! `…Ts` types, never built) that fields point at with `#[ts(as = "…")]`. Ids are `number`.
//! Every type is exported into one file, `types.ts`.

use airboss_core::{
    EventId, EventKind, LinkId, LinkKind, Origin, PrState, Priority, Repo, RepoId, Status, Task,
    TaskId,
};
use airboss_services::{
    Availability, Chip, Connection, HerdrAgent, LaunchReport, Launcher, NeedsItem, NeedsList,
    PrView, Prepared, Request, Source, ThreadKey, ThreadState, ThreadView,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::errors::AppError;
use crate::settings::Settings;

/// UTC text in `core`'s format, `"2026-11-25T17:19:07.841Z"`.
pub type Timestamp = String;

// ================================================================ TypeScript-only mirrors
//
// These are never constructed: they only give ts-rs the shape `core` and `crates/services`
// already serialise. Keep them in step with those crates' serde attributes.

macro_rules! ts_alias {
    ($($(#[$m:meta])* $rust:ident = $ts:literal : $inner:ty;)*) => {$(
        $(#[$m])*
        #[derive(TS)]
        #[ts(rename = $ts, export_to = "types.ts")]
        #[allow(dead_code)]
        pub struct $rust($inner);
    )*};
}

ts_alias! {
    /// A task's id.
    TaskIdTs = "TaskId": f64;
    /// A repo's id.
    RepoIdTs = "RepoId": f64;
    /// A link's id.
    LinkIdTs = "LinkId": f64;
    /// An event's id.
    EventIdTs = "EventId": f64;
    /// UTC text in `core`'s format.
    TimestampTs = "Timestamp": String;
    /// "me", "agent:<thread ref>", "sync:gh", "sync:t3", "plan" or "launch".
    ActorTs = "Actor": String;
    /// e.g. "question:<request id>"; stable across restarts.
    NeedsKeyTs = "NeedsKey": String;
    /// "owner/repo#n", lowercase.
    PrRefTs = "PrRef": String;
}

/// `core::Status`.
#[derive(TS)]
#[ts(rename = "Status", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum StatusTs {
    Proposed,
    Todo,
    Doing,
    InReview,
    Blocked,
    Done,
    Dropped,
}

/// `core::Origin`.
#[derive(TS)]
#[ts(rename = "Origin", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum OriginTs {
    Me,
    Agent,
    Plan,
}

/// `core::EventKind`.
#[derive(TS)]
#[ts(
    rename = "EventKind",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum EventKindTs {
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
    Rejected,
    RepoLinked,
    RepoUnlinked,
}

/// `core::LinkKind`.
#[derive(TS)]
#[ts(rename = "LinkKind", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum LinkKindTs {
    T3Thread,
    ClaudeSession,
    Pr,
    Slack,
    Doc,
    GithubIssue,
    HerdrTerminal,
}

/// `core::PrState`.
#[derive(TS)]
#[ts(rename = "PrState", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum PrStateTs {
    Open,
    Merged,
    Closed,
}

/// `core::Priority`: p0 is the most urgent.
#[derive(TS)]
#[ts(rename = "Priority", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum PriorityTs {
    P0,
    P1,
    P2,
    P3,
}

/// `core::Task`.
#[derive(TS)]
#[ts(rename = "Task", export_to = "types.ts")]
#[allow(dead_code)]
pub struct TaskTs {
    id: TaskIdTs,
    parent: Option<TaskIdTs>,
    repos: Vec<RepoIdTs>,
    title: String,
    body: Option<String>,
    status: StatusTs,
    blocked_reason: Option<String>,
    position: u32,
    origin: OriginTs,
    plan_key: Option<String>,
    branch: Option<String>,
    priority: Option<PriorityTs>,
    created_at: TimestampTs,
}

/// `core::Repo`.
#[derive(TS)]
#[ts(rename = "Repo", export_to = "types.ts")]
#[allow(dead_code)]
pub struct RepoTs {
    id: RepoIdTs,
    name: String,
    path: String,
    remote_url: Option<String>,
}

/// `core::Event`.
#[derive(TS)]
#[ts(rename = "Event", export_to = "types.ts")]
#[allow(dead_code)]
pub struct EventTs {
    id: EventIdTs,
    task: TaskIdTs,
    kind: EventKindTs,
    to_status: Option<StatusTs>,
    body: Option<String>,
    actor: ActorTs,
    at: TimestampTs,
}

/// `core::Link`.
#[derive(TS)]
#[ts(rename = "Link", export_to = "types.ts")]
#[allow(dead_code)]
pub struct LinkTs {
    id: LinkIdTs,
    task: TaskIdTs,
    kind: LinkKindTs,
    r#ref: String,
    url: Option<String>,
    pr_state: Option<PrStateTs>,
    title: Option<String>,
    linked_by: ActorTs,
    linked_at: TimestampTs,
}

/// `services::Connection`.
#[derive(TS)]
#[ts(rename = "Connection", export_to = "types.ts")]
#[allow(dead_code)]
pub struct ConnectionTs {
    name: String,
    ok: bool,
    state: String,
    detail: String,
    fix: Option<String>,
}

/// `services::ErrorCode`: a service's error codes.
#[derive(TS)]
#[ts(
    rename = "ErrorCode",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum ServiceErrorCodeTs {
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
    HerdrMissing,
    HerdrNotRunning,
    WorktreeFailed,
    HerdrFailed,
    GhMissing,
    SignedOut,
    NoAccess,
    PrNotFound,
    BadPrRef,
    RateLimited,
    GhFailed,
    T3Unavailable,
    NotFound,
    Core,
    Storage,
    Internal,
    ShuttingDown,
}

/// `services::ServiceError`.
#[derive(TS)]
#[ts(rename = "ServiceError", export_to = "types.ts")]
#[allow(dead_code)]
pub struct ServiceErrorTs {
    code: ServiceErrorCodeTs,
    message: String,
}

/// `services::Source`.
#[derive(TS)]
#[ts(rename = "Source", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum SourceTs {
    T3,
    Claude,
    Herdr,
}

/// `services::ThreadKey`.
#[derive(TS)]
#[ts(rename = "ThreadKey", export_to = "types.ts")]
#[allow(dead_code)]
pub struct ThreadKeyTs {
    source: SourceTs,
    r#ref: String,
}

/// `services::ThreadState`.
#[derive(TS)]
#[ts(
    rename = "ThreadState",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum ThreadStateTs {
    Running,
    NeedsMe,
    Errored,
    Idle,
    Settled,
    Gone,
}

/// `services::Chip`.
#[derive(TS)]
#[ts(rename = "Chip", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum ChipTs {
    Working,
    WaitingOnYou,
    YourTurn,
    Stopped,
}

/// `services::RequestKind`.
#[derive(TS)]
#[ts(
    rename = "RequestKind",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum RequestKindTs {
    Question,
    Approval,
}

/// `services::Request`.
#[derive(TS)]
#[ts(rename = "Request", export_to = "types.ts")]
#[allow(dead_code)]
pub struct RequestTs {
    id: String,
    kind: RequestKindTs,
    text: String,
    at: TimestampTs,
}

/// `services::ThreadView`.
#[derive(TS)]
#[ts(rename = "ThreadView", export_to = "types.ts")]
#[allow(dead_code)]
pub struct ThreadViewTs {
    key: ThreadKeyTs,
    linked: bool,
    state: ThreadStateTs,
    title: Option<String>,
    request: Option<RequestTs>,
    error: Option<String>,
    run_id: Option<String>,
    project_path: Option<String>,
    branch: Option<String>,
    worktree_path: Option<String>,
    parent_ref: Option<String>,
    child_count: u32,
    last_activity_at: Option<TimestampTs>,
    last_reply: Option<String>,
    stale: bool,
}

/// `services::Review`.
#[derive(TS)]
#[ts(rename = "Review", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum ReviewTs {
    Approved,
    ChangesRequested,
    Required,
    None,
}

/// `services::Checks`.
#[derive(TS)]
#[ts(rename = "Checks", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum ChecksTs {
    Passing,
    Failing,
    Pending,
    None,
}

/// `services::PrProblem`.
#[derive(TS)]
#[ts(
    rename = "PrProblem",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum PrProblemTs {
    NotFound,
    NoAccess,
    Failed(String),
}

/// `services::PrView`.
#[derive(TS)]
#[ts(rename = "PrView", export_to = "types.ts")]
#[allow(dead_code)]
pub struct PrViewTs {
    r#ref: PrRefTs,
    linked: bool,
    url: Option<String>,
    title: Option<String>,
    state: Option<PrStateTs>,
    draft: bool,
    review: ReviewTs,
    checks: ChecksTs,
    additions: Option<u32>,
    deletions: Option<u32>,
    head_ref: Option<String>,
    author: Option<String>,
    updated_at: Option<TimestampTs>,
    ready_since: Option<TimestampTs>,
    problem: Option<PrProblemTs>,
    stale: bool,
}

/// `services::T3Project`.
#[derive(TS)]
#[ts(rename = "T3Project", export_to = "types.ts")]
#[allow(dead_code)]
pub struct T3ProjectTs {
    id: String,
    name: String,
    path: String,
}

/// `services::Availability`.
#[derive(TS)]
#[ts(
    rename = "Availability",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum AvailabilityTs {
    Unknown,
    Ready { version: String },
    Missing { reason: String },
}

/// `services::HerdrAgent`: the agent a Herdr tab runs.
#[derive(TS)]
#[ts(
    rename = "HerdrAgent",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum HerdrAgentTs {
    Claude,
    Codex,
}

/// Where Do it and Plan it start agents now: "Start agents in", resolved against what's Ready
/// (`Settings::launcher`). The buttons, their disabled reason and the copy follow it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum LauncherKind {
    #[default]
    T3,
    Herdr,
}

impl From<Launcher> for LauncherKind {
    fn from(l: Launcher) -> LauncherKind {
        match l {
            Launcher::T3 => LauncherKind::T3,
            Launcher::Herdr { .. } => LauncherKind::Herdr,
        }
    }
}

/// `services::Mode`.
#[derive(TS)]
#[ts(rename = "Mode", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum ModeTs {
    DoIt,
    PlanIt,
}

/// `services::StartedThread`.
#[derive(TS)]
#[ts(rename = "StartedThread", export_to = "types.ts")]
#[allow(dead_code)]
pub struct StartedThreadTs {
    key: ThreadKeyTs,
    mode: ModeTs,
    repo: Option<RepoIdTs>,
    title: String,
    linked: bool,
}

/// `services::Prepared`.
#[derive(TS)]
#[ts(rename = "Prepared", export_to = "types.ts")]
#[allow(dead_code)]
pub struct PreparedTs {
    repos: Vec<RepoTs>,
    from_subtasks: bool,
    needs_choice: bool,
    choices: Vec<RepoTs>,
    branch: Option<String>,
    research_dir: String,
    waiting_on: Vec<TaskIdTs>,
    started: Vec<StartedThreadTs>,
}

/// `services::LaunchReport`.
#[derive(TS)]
#[ts(rename = "LaunchReport", export_to = "types.ts")]
#[allow(dead_code)]
pub struct LaunchReportTs {
    started: Vec<StartedThreadTs>,
    skipped: Vec<RepoIdTs>,
    failed: Vec<(Option<RepoIdTs>, ServiceErrorTs)>,
    branch: Option<String>,
    moved_to_doing: bool,
}

/// `services::NeedsKind`.
#[derive(TS)]
#[ts(
    rename = "NeedsKind",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum NeedsKindTs {
    Question,
    Approval,
    Review,
    FailedRun,
    AgentStopped,
    Proposed,
    Ready,
    Quiet,
}

/// `services::NeedsGroup`, in display order.
#[derive(TS)]
#[ts(
    rename = "NeedsGroup",
    rename_all = "snake_case",
    export_to = "types.ts"
)]
#[allow(dead_code)]
pub enum NeedsGroupTs {
    Questions,
    Reviews,
    FailedRuns,
    Stopped,
    Proposed,
    Ready,
    Quiet,
}

/// `services::Target`.
#[derive(TS)]
#[ts(rename = "Target", rename_all = "snake_case", export_to = "types.ts")]
#[allow(dead_code)]
pub enum TargetTs {
    Thread(ThreadKeyTs),
    Pr { r#ref: PrRefTs, url: Option<String> },
    Task(TaskIdTs),
}

/// `services::NeedsItem`.
#[derive(TS)]
#[ts(rename = "NeedsItem", export_to = "types.ts")]
#[allow(dead_code)]
pub struct NeedsItemTs {
    key: NeedsKeyTs,
    kind: NeedsKindTs,
    group: NeedsGroupTs,
    task: TaskIdTs,
    task_title: String,
    since: TimestampTs,
    headline: String,
    context: String,
    target: TargetTs,
    /// Proposed only: [task id, title].
    proposed: Vec<(TaskIdTs, String)>,
    stale: bool,
}

/// `services::NeedsList`.
#[derive(TS)]
#[ts(rename = "NeedsList", export_to = "types.ts")]
#[allow(dead_code)]
pub struct NeedsListTs {
    items: Vec<NeedsItemTs>,
    count: u32,
    checked_at: Option<TimestampTs>,
    version: f64,
}

/// `needs:changed`'s payload: the Needs me list.
#[derive(TS)]
#[ts(rename = "NeedsChanged", export_to = "types.ts")]
#[allow(dead_code)]
pub struct NeedsChangedTs {
    version: f64,
    items: Vec<NeedsItemTs>,
    count: u32,
    checked_at: Option<TimestampTs>,
}

// ================================================================ rows and patches

/// Leaf subtasks at any depth: dropped ones left out, proposed ones counted as todo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct Progress {
    pub done: u32,
    pub total: u32,
    pub in_review: u32,
    pub doing: u32,
    pub blocked: u32,
}

impl Progress {
    /// Adds another count.
    pub fn add(&mut self, o: Progress) {
        self.done += o.done;
        self.total += o.total;
        self.in_review += o.in_review;
        self.doing += o.doing;
        self.blocked += o.blocked;
    }

    /// Whole percent done, rounded down; 0 with nothing to count.
    pub fn percent(self) -> u32 {
        self.done
            .saturating_mul(100)
            .checked_div(self.total)
            .unwrap_or(0)
    }
}

/// Neither done nor dropped.
pub fn open(s: Status) -> bool {
    !matches!(s, Status::Done | Status::Dropped)
}

/// The design's label for a status, for undo entries and toasts.
pub fn status_label(s: Status) -> &'static str {
    match s {
        Status::Proposed => "Proposed",
        Status::Todo => "Todo",
        Status::Doing => "In progress",
        Status::InReview => "In review",
        Status::Blocked => "Blocked",
        Status::Done => "Done",
        Status::Dropped => "Dropped",
    }
}

/// Higher is stronger: Waiting on you, then Working, then Your turn, then Stopped.
pub fn chip_strength(c: Chip) -> u8 {
    match c {
        Chip::WaitingOnYou => 4,
        Chip::Working => 3,
        Chip::YourTurn => 2,
        Chip::Stopped => 1,
    }
}

/// One row of the task list: everything it draws, worked out in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct RowDto {
    #[ts(as = "TaskIdTs")]
    pub id: TaskId,
    #[ts(as = "Option<TaskIdTs>")]
    pub parent: Option<TaskId>,
    pub depth: u32,
    pub position: u32,
    pub title: String,
    #[ts(as = "StatusTs")]
    pub status: Status,
    #[ts(as = "OriginTs")]
    pub origin: Origin,
    /// Its own priority, P0 to P3, for the badge; `null` when none is set.
    #[ts(as = "Option<PriorityTs>")]
    pub priority: Option<Priority>,
    pub blocked_reason: Option<String>,
    /// Something it waits on is still open.
    pub waiting: bool,
    /// The open tasks it waits on (empty unless `waiting`).
    #[ts(as = "Vec<TaskIdTs>")]
    pub waits_on: Vec<TaskId>,
    pub has_children: bool,
    /// It has steps, subtasks that aren't proposals: its status is worked out from them, and
    /// it takes no status change but Drop, no branch and no PR (SPEC-core rule 10).
    pub has_steps: bool,
    /// Over leaf subtasks at any depth; `null` for a leaf.
    pub progress: Option<Progress>,
    /// Its own repos, or for a parent with none the union over its subtree.
    #[ts(as = "Vec<RepoIdTs>")]
    pub repos: Vec<RepoId>,
    pub repos_from_subtasks: bool,
    /// Its branch, or the nearest ancestor's (display only).
    pub branch: Option<String>,
    /// In `core`'s ready list, and a leaf.
    pub ready: bool,
    #[ts(as = "Option<TimestampTs>")]
    pub ready_since: Option<Timestamp>,
    /// `attention` lists it as Ready to start.
    pub ready_pill: bool,
    /// `attention` lists it as Quiet: whole days since its last activity.
    pub quiet_days: Option<u32>,
    /// Any Needs me item about it (the red dot).
    pub needs: bool,
    /// Any of those items may be out of date.
    pub needs_stale: bool,
    /// The strongest chip of its threads, only while it's open.
    #[ts(as = "Option<ChipTs>")]
    pub live: Option<Chip>,
    /// Its newest event, or for a parent the newest anywhere in its subtree.
    #[ts(as = "TimestampTs")]
    pub last_at: Timestamp,
    /// Who made that event, as text ("me", "agent:t3/…").
    #[ts(as = "ActorTs")]
    pub last_actor: String,
    /// When `last_actor` is Launch: where its thread runs (T3 or Herdr), so the avatar can say
    /// so. The task's most recently active T3 thread or Herdr terminal; `null` otherwise.
    #[ts(as = "Option<SourceTs>")]
    pub launched_in: Option<Source>,
}

/// What changed in the rows since the last patch. Apply it only if `seq` is greater than the
/// last one applied: a write's returned patch and the same patch as an event apply once.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct Patch {
    #[ts(as = "f64")]
    pub seq: u64,
    /// Whole rows, new or changed.
    pub upserts: Vec<RowDto>,
    #[ts(as = "Vec<TaskIdTs>")]
    pub removed: Vec<TaskId>,
    /// Every id in tree order, sent only when it changed.
    #[ts(as = "Option<Vec<TaskIdTs>>")]
    pub order: Option<Vec<TaskId>>,
    /// Every repo, sent only when they changed.
    #[ts(as = "Option<Vec<RepoTs>>")]
    pub repos: Option<Vec<Repo>>,
    /// New tasks, for the new-row highlight.
    #[ts(as = "Vec<TaskIdTs>")]
    pub created: Vec<TaskId>,
}

impl Patch {
    /// A patch with nothing in it and `seq` 0, below every real patch's, so the UI (which
    /// applies a patch only if its `seq` is greater than the last one applied) skips it.
    pub fn none() -> Patch {
        Patch::default()
    }

    /// Nothing in it.
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty()
            && self.removed.is_empty()
            && self.order.is_none()
            && self.repos.is_none()
    }
}

// ================================================================ bootstrap and connections

/// Whether the `airboss` command-line tool is linked, for Settings and the Agents row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct CliStatus {
    pub installed: bool,
    pub path: String,
    pub on_path: bool,
}

/// Which agent tools have the `airboss` MCP server, read from their own config files
/// (`~/.claude.json`, `~/.codex/config.toml`), read-only. Agents are Ready only with one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct AgentTools {
    /// `~/.claude.json` has `airboss` in its top-level `mcpServers`.
    pub claude: bool,
    /// `~/.codex/config.toml` has `[mcp_servers.airboss]`.
    pub codex: bool,
    /// That table's `env_vars` passes `HC_TERMINAL_ID`, which Codex in Herdr needs to say
    /// which terminal it is. A setup copied before Herdr has only `T3CODE_THREAD_ID`.
    pub codex_terminal_id: bool,
}

impl AgentTools {
    /// Codex runs in Herdr (`launcher` is Herdr and its agent is Codex) with an `airboss`
    /// table that doesn't pass `HC_TERMINAL_ID`, so its agents can't name their terminal.
    pub fn codex_needs_terminal_id(&self, launcher: LauncherKind, agent: HerdrAgent) -> bool {
        launcher == LauncherKind::Herdr
            && agent == HerdrAgent::Codex
            && self.codex
            && !self.codex_terminal_id
    }
}

/// Everything the main window draws first, read once before it shows.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct Bootstrap {
    #[ts(as = "f64")]
    pub seq: u64,
    /// In tree order.
    pub rows: Vec<RowDto>,
    #[ts(as = "Vec<TaskIdTs>")]
    pub order: Vec<TaskId>,
    #[ts(as = "Vec<RepoTs>")]
    pub repos: Vec<Repo>,
    #[ts(as = "NeedsListTs")]
    pub needs: NeedsList,
    #[ts(as = "Vec<ConnectionTs>")]
    pub connections: Vec<Connection>,
    #[ts(as = "AvailabilityTs")]
    pub availability: Availability,
    /// Whether Herdr is there to start agents in.
    #[ts(as = "AvailabilityTs")]
    pub herdr: Availability,
    /// Where Do it and Plan it start agents now.
    pub launcher: LauncherKind,
    /// When an agent last reported (the newest `agent:*` event), for the Agents row.
    #[ts(as = "Option<TimestampTs>")]
    pub agent_report: Option<Timestamp>,
    pub settings: Settings,
    /// The system accent as `#rrggbb`, or `null` for Multicolor (keep indigo).
    pub accent: Option<String>,
    pub cli: CliStatus,
    pub agents: AgentTools,
}

/// `connections:changed`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct ConnectionsChanged {
    #[ts(as = "Vec<ConnectionTs>")]
    pub connections: Vec<Connection>,
    #[ts(as = "AvailabilityTs")]
    pub availability: Availability,
    /// Whether Herdr is there to start agents in.
    #[ts(as = "AvailabilityTs")]
    pub herdr: Availability,
    /// Where Do it and Plan it start agents now: "Start agents in", resolved.
    pub launcher: LauncherKind,
    /// When an agent last reported (the newest `agent:*` event), for "Last report 3m ago".
    #[ts(as = "Option<TimestampTs>")]
    pub agent_report: Option<Timestamp>,
    /// The command-line tool, which the Agents row needs; `on_path` arrives once the shell
    /// check is done.
    pub cli: CliStatus,
    /// Which agent tools can reach Airboss, for the Agents row and Settings › Agent setup.
    pub agents: AgentTools,
}

// ================================================================ task detail

/// One thread linked to a task, as `sync` last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct ThreadDto {
    #[ts(as = "ThreadKeyTs")]
    pub key: ThreadKey,
    #[ts(as = "ThreadStateTs")]
    pub state: ThreadState,
    #[ts(as = "Option<ChipTs>")]
    pub chip: Option<Chip>,
    pub title: Option<String>,
    #[ts(as = "Option<RequestTs>")]
    pub request: Option<Request>,
    pub error: Option<String>,
    pub project_path: Option<String>,
    /// `t3code://threads/…`; `null` for a Claude Code session, which shows its folder.
    pub url: Option<String>,
    #[ts(as = "Option<TimestampTs>")]
    pub last_activity_at: Option<Timestamp>,
    /// The agent's newest reply (T3), where it says what's left for me.
    pub last_reply: Option<String>,
    pub stale: bool,
}

impl ThreadDto {
    /// The thread, with the URL `sync.thread_url` gave for it.
    pub fn new(t: &ThreadView, url: Option<String>) -> ThreadDto {
        ThreadDto {
            key: t.key.clone(),
            state: t.state,
            chip: t.state.chip(),
            title: t.title.clone(),
            request: t.request.clone(),
            error: t.error.clone(),
            project_path: t.project_path.clone(),
            url,
            last_activity_at: t.last_activity_at.clone(),
            last_reply: t.last_reply.clone(),
            stale: t.stale,
        }
    }
}

/// One link on a task (a `Link`), with what `sync` knows about its PR or thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct LinkDto {
    #[ts(as = "LinkIdTs")]
    pub id: LinkId,
    #[ts(as = "TaskIdTs")]
    pub task: TaskId,
    #[ts(as = "LinkKindTs")]
    pub kind: LinkKind,
    pub r#ref: String,
    pub url: Option<String>,
    #[ts(as = "Option<PrStateTs>")]
    pub pr_state: Option<PrState>,
    pub title: Option<String>,
    #[ts(as = "ActorTs")]
    pub linked_by: String,
    #[ts(as = "TimestampTs")]
    pub linked_at: Timestamp,
    #[ts(as = "Option<PrViewTs>")]
    pub pr: Option<PrView>,
    pub thread: Option<ThreadDto>,
}

/// One event in the activity feed (an `Event`), with the agent's thread title when known
/// ("Agent in slot-picker").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct HistoryEntry {
    #[ts(as = "EventIdTs")]
    pub id: EventId,
    #[ts(as = "TaskIdTs")]
    pub task: TaskId,
    #[ts(as = "EventKindTs")]
    pub kind: EventKind,
    #[ts(as = "Option<StatusTs>")]
    pub to_status: Option<Status>,
    pub body: Option<String>,
    #[ts(as = "ActorTs")]
    pub actor: String,
    #[ts(as = "TimestampTs")]
    pub at: Timestamp,
    pub thread_title: Option<String>,
}

/// Another task in a Waits on or Blocks list, with its status icon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct DepDto {
    #[ts(as = "TaskIdTs")]
    pub id: TaskId,
    pub title: String,
    #[ts(as = "StatusTs")]
    pub status: Status,
    pub waiting: bool,
}

/// A line from a blocker to the task waiting on it, lifted to the parent's subtasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct RunEdge {
    /// The blocker.
    #[ts(as = "TaskIdTs")]
    pub from: TaskId,
    /// The task that waits.
    #[ts(as = "TaskIdTs")]
    pub to: TaskId,
    /// Every blocker behind it is done or dropped: a solid line.
    pub done: bool,
}

/// A subtask held by open tasks outside its parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct OutsideWait {
    #[ts(as = "TaskIdTs")]
    pub task: TaskId,
    #[ts(as = "Vec<TaskIdTs>")]
    pub blockers: Vec<TaskId>,
}

/// Display waves (done subtasks included), lifted edges, outside blockers and loops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct RunMap {
    /// The stations, in sibling order.
    #[ts(as = "Vec<TaskIdTs>")]
    pub nodes: Vec<TaskId>,
    pub edges: Vec<RunEdge>,
    #[ts(as = "Vec<Vec<TaskIdTs>>")]
    pub waves: Vec<Vec<TaskId>>,
    pub outside: Vec<OutsideWait>,
    #[ts(as = "Vec<TaskIdTs>")]
    pub tangled: Vec<TaskId>,
}

/// Everything the task panel and page show for one task.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct TaskDetail {
    /// The request number this answers; the UI ignores a stale one.
    pub req: u32,
    #[ts(as = "TaskTs")]
    pub task: Task,
    pub links: Vec<LinkDto>,
    /// Newest first, from the task and everything under it.
    pub history: Vec<HistoryEntry>,
    /// The history was cut at the limit; "Show more" asks for 200.
    pub history_more: bool,
    pub waits_on: Vec<DepDto>,
    pub blocks: Vec<DepDto>,
    /// For a task with subtasks: its display waves.
    pub waves: Option<RunMap>,
    #[ts(as = "Vec<NeedsItemTs>")]
    pub needs: Vec<NeedsItem>,
    /// What Do it would do; `null` for proposed, done and dropped tasks (or when launch can't
    /// say, e.g. T3 is missing).
    #[ts(as = "Option<PreparedTs>")]
    pub prepared: Option<Prepared>,
    /// My open PRs from this task's branch, to link with one click.
    #[ts(as = "Vec<PrViewTs>")]
    pub suggested_prs: Vec<PrView>,
}

// ================================================================ plans and quick add

/// A ready subtask a plan card lists under Next up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct NextUp {
    #[ts(as = "TaskIdTs")]
    pub id: TaskId,
    pub title: String,
}

/// One card on Plans: an open top-level parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct PlanCard {
    #[ts(as = "TaskIdTs")]
    pub id: TaskId,
    pub title: String,
    #[ts(as = "StatusTs")]
    pub status: Status,
    pub waiting: bool,
    pub progress: Progress,
    /// How many display waves.
    pub waves: u32,
    /// 0-based: the lowest wave with an open task; `null` when every wave is done.
    pub current_wave: Option<u32>,
    /// Up to two ready subtasks.
    pub next_up: Vec<NextUp>,
    /// Subtasks with a working agent.
    pub working: u32,
    pub blocked: u32,
    #[ts(as = "Vec<RepoIdTs>")]
    pub repos: Vec<RepoId>,
    #[ts(as = "TimestampTs")]
    pub last_at: Timestamp,
}

/// An open task quick add can put a new task under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct ParentOption {
    #[ts(as = "TaskIdTs")]
    pub id: TaskId,
    pub title: String,
    #[ts(as = "StatusTs")]
    pub status: Status,
    pub waiting: bool,
    pub depth: u32,
}

/// What quick add needs: the repos and the open tasks, in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct QuickaddData {
    #[ts(as = "Vec<RepoTs>")]
    pub repos: Vec<Repo>,
    pub parents: Vec<ParentOption>,
}

/// One of T3's projects, or a folder Herdr knows (`herdr_folders`), for Add repos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct T3ProjectRow {
    pub path: String,
    pub name: String,
    /// Has a `.git`; a folder that isn't is shown greyed.
    pub is_git: bool,
    /// Already a repo here.
    pub added: bool,
}

// ================================================================ writes

/// An undo entry, so a toast can undo exactly the action it reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct UndoEntry {
    #[ts(as = "f64")]
    pub id: u64,
    /// e.g. "Mark done #144".
    pub label: String,
}

/// A write that may be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct UndoableWrite {
    /// `null` for writes that touch no row yet (a deferred reject, a dismiss).
    pub patch: Option<Patch>,
    /// `null` when the write changed nothing.
    pub undo: Option<UndoEntry>,
}

/// `undo`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct UndoResult {
    pub patch: Option<Patch>,
    pub message: String,
}

/// One link pasted into New task: the text and an optional title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct NewLinkText {
    pub text: String,
    pub title: Option<String>,
}

/// What `create_task` does after creating.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum Then {
    #[default]
    None,
    DoIt,
    PlanIt,
}

/// New task's fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct CreateTaskInput {
    pub title: String,
    pub body: Option<String>,
    #[ts(as = "Option<TaskIdTs>")]
    pub parent: Option<TaskId>,
    #[ts(as = "Vec<RepoIdTs>")]
    #[serde(default)]
    pub repos: Vec<RepoId>,
    #[serde(default)]
    pub links: Vec<NewLinkText>,
    #[serde(default)]
    pub then: Then,
}

/// Where Do it starts, from the Start in menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum DoItPlace {
    /// The task's own repos (or its subtasks'). The default.
    Linked,
    /// "Start in <repo>": link it as me, then start there.
    Repo(#[ts(as = "RepoIdTs")] RepoId),
    /// "No repo: research only".
    NoRepo,
}

/// What Do it or Plan it did.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum DoItResult {
    /// The task has no repo: ask where (the Start in menu).
    NeedsChoice {
        #[ts(as = "Vec<RepoTs>")]
        choices: Vec<Repo>,
        research_dir: String,
    },
    /// `start` ran; the report says place by place what happened.
    Started {
        #[ts(as = "LaunchReportTs")]
        report: LaunchReport,
        patch: Patch,
    },
}

/// A link that couldn't be added after the task existed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct LinkError {
    pub text: String,
    pub error: AppError,
}

/// `create_task`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct CreateTaskResult {
    #[ts(as = "TaskIdTs")]
    pub id: TaskId,
    pub patch: Patch,
    /// When `then` asked for Do it or Plan it and it ran; if it failed, a `toast` says why.
    pub launch: Option<DoItResult>,
    /// "Created #153, but couldn't link …".
    pub link_errors: Vec<LinkError>,
}

/// What `remove_repo` answers: the tasks that lost the link, and the patch.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct RemoveRepoResult {
    #[ts(as = "Vec<TaskIdTs>")]
    pub unlinked: Vec<TaskId>,
    pub patch: Patch,
}

/// `add_repos`' answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct AddReposResult {
    /// The repos just added.
    #[ts(as = "Vec<RepoTs>")]
    pub repos: Vec<Repo>,
    pub patch: Patch,
}

/// `install_cli`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct InstallCliResult {
    pub path: String,
    /// `~/.local/bin` is on the login shell's `PATH`.
    pub on_path: bool,
}

/// What to copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum CopyWhat {
    TaskNumber(#[ts(as = "TaskIdTs")] TaskId),
    AgentNote,
    SetupCommand,
    /// The `[mcp_servers.airboss]` table for `~/.codex/config.toml`.
    CodexSetup,
    /// A link Airboss wouldn't open ("Copy link" on the refusal).
    Link(String),
}

/// What to open: a URL, or a link by its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum OpenTarget {
    Url(String),
    Link(#[ts(as = "LinkIdTs")] LinkId),
}

// ================================================================ events

/// `menu:action`'s payload: a command id from `lib/logic/commands.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct MenuAction {
    pub command: String,
}

/// `nav:open`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct NavOpen {
    pub view: Option<String>,
    #[ts(as = "Option<TaskIdTs>")]
    pub task: Option<TaskId>,
    #[ts(as = "Option<NeedsKeyTs>")]
    pub needs_key: Option<String>,
}

/// `quickadd:reset`'s payload: the panel hid; reset the form and refetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct QuickaddReset {
    /// The hotkey as the header shows it, e.g. "⌃⌥N".
    pub hotkey: String,
    /// Hidden by a click elsewhere: keep a non-empty title and notes.
    #[ts(optional)]
    pub keep_text: Option<bool>,
}

/// What a toast's button calls: the `api` function for `command`, with `args` in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ToastArgs {
    /// `do_it(id, place, again)`.
    DoIt {
        id: TaskId,
        place: Option<DoItPlace>,
        again: bool,
    },
    /// `plan_it(id, again)`.
    PlanIt { id: TaskId, again: bool },
    /// `open_thread(key)`.
    Thread { key: ThreadKey },
    /// `add_to_t3(repo)`.
    Repo { repo: RepoId },
    /// `reopen_and_start(id)`.
    Task { id: TaskId },
    /// `reject_proposals(ids, reason)`: Retry on a reject that couldn't be written.
    Reject {
        ids: Vec<TaskId>,
        reason: Option<String>,
    },
    /// No arguments (`{}`), e.g. `install_update()`.
    Empty {},
}

/// One button on a toast Rust sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct ToastEventAction {
    pub label: String,
    /// A command name, e.g. "do_it".
    pub command: String,
    #[ts(type = "Record<string, unknown>")]
    pub args: ToastArgs,
}

/// A toast's tone: plain, or with the orange warning icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum ToastTone {
    Info,
    Warning,
}

/// `toast`'s payload: a toast the UI didn't ask for (a Do it from quick add that failed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "types.ts")]
pub struct ToastEvent {
    pub text: String,
    pub actions: Vec<ToastEventAction>,
    pub undo: Option<UndoEntry>,
    #[ts(optional)]
    pub tone: Option<ToastTone>,
    #[ts(optional, as = "Option<TaskIdTs>")]
    pub task: Option<TaskId>,
}

/// The theme the windows follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum Theme {
    Dark,
    Light,
}

/// In-app updates (SPEC-updates): what the sidebar's update card shows, and
/// `update:changed`'s payload. `Idle` is nothing to show: not checked yet, up to date, or
/// automatic checks off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum UpdateStatus {
    Idle,
    /// A newer release; `notes_url` is its GitHub release page.
    Available {
        version: String,
        notes_url: String,
    },
    /// `percent` once the download's size is known.
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    Installing {
        version: String,
    },
    Restarting {
        version: String,
    },
    /// The update stopped before the swap; the running app is untouched.
    Failed {
        version: String,
        message: String,
    },
}

/// `appearance:changed`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct AppearanceChanged {
    pub accent: Option<String>,
    pub theme: Theme,
}

/// `error:background`'s payload: the watcher or an app thread failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct BackgroundError {
    pub source: String,
    pub message: String,
}

/// A path as text for the UI.
pub fn path_text(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}
