//! Every type the UI sees, mirroring `core` and the services, with TypeScript types from ts-rs.
//!
//! `core` and `crates/services` don't depend on ts-rs, so each type the UI reads has a mirror
//! here with a `From` conversion. Ids and `seq` are `number` in TypeScript (they stay far below
//! 2^53). Field names are snake_case on both sides.

use std::path::PathBuf;

use airboss_core::{
    Actor, Event, EventId, EventKind, Link, LinkId, LinkKind, Origin, PrState, Repo, RepoId,
    Status, Task, TaskId,
};
use airboss_services::{
    Availability, Checks, Chip, Connection, LaunchReport, Mode, NeedsGroup, NeedsItem, NeedsKind,
    NeedsList, PrView, Prepared, Request, RequestKind, Review, ServiceError, Source, StartedThread,
    Target, ThreadKey, ThreadState, ThreadView,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::errors::AppError;
use crate::settings::Settings;

/// UTC text in `core`'s format, `"2026-11-25T17:19:07.841Z"`.
pub type Timestamp = String;

// ---------------------------------------------------------------------------- enums

/// A task's status (`core`'s, as stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StatusDto {
    Proposed,
    Todo,
    Doing,
    InReview,
    Blocked,
    Done,
    Dropped,
}

impl From<Status> for StatusDto {
    fn from(s: Status) -> StatusDto {
        match s {
            Status::Proposed => StatusDto::Proposed,
            Status::Todo => StatusDto::Todo,
            Status::Doing => StatusDto::Doing,
            Status::InReview => StatusDto::InReview,
            Status::Blocked => StatusDto::Blocked,
            Status::Done => StatusDto::Done,
            Status::Dropped => StatusDto::Dropped,
        }
    }
}

impl From<StatusDto> for Status {
    fn from(s: StatusDto) -> Status {
        match s {
            StatusDto::Proposed => Status::Proposed,
            StatusDto::Todo => Status::Todo,
            StatusDto::Doing => Status::Doing,
            StatusDto::InReview => Status::InReview,
            StatusDto::Blocked => Status::Blocked,
            StatusDto::Done => Status::Done,
            StatusDto::Dropped => Status::Dropped,
        }
    }
}

impl StatusDto {
    /// Neither done nor dropped.
    pub fn is_open(self) -> bool {
        !matches!(self, StatusDto::Done | StatusDto::Dropped)
    }

    /// The design's label, for undo entries and toasts.
    pub fn label(self) -> &'static str {
        match self {
            StatusDto::Proposed => "Proposed",
            StatusDto::Todo => "Todo",
            StatusDto::Doing => "In progress",
            StatusDto::InReview => "In review",
            StatusDto::Blocked => "Blocked",
            StatusDto::Done => "Done",
            StatusDto::Dropped => "Dropped",
        }
    }
}

/// How a task came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OriginDto {
    Me,
    Agent,
    Plan,
}

impl From<Origin> for OriginDto {
    fn from(o: Origin) -> OriginDto {
        match o {
            Origin::Me => OriginDto::Me,
            Origin::Agent => OriginDto::Agent,
            Origin::Plan => OriginDto::Plan,
        }
    }
}

/// Who made a change: the avatar's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Me,
    Agent,
    SyncGh,
    SyncT3,
    Plan,
    Launch,
}

impl From<&Actor> for ActorKind {
    fn from(a: &Actor) -> ActorKind {
        match a {
            Actor::Me => ActorKind::Me,
            Actor::Agent(_) => ActorKind::Agent,
            Actor::SyncGh => ActorKind::SyncGh,
            Actor::SyncT3 => ActorKind::SyncT3,
            Actor::Plan => ActorKind::Plan,
            Actor::Launch => ActorKind::Launch,
        }
    }
}

/// Who made a change, with the name the activity feed shows ("You", "Agent in slot-picker" …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActorDto {
    pub kind: ActorKind,
    pub name: String,
    /// An agent's thread ref, e.g. `"t3/97b5c8b5"`.
    pub thread: Option<String>,
}

/// What kind of change an event records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EventKindDto {
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

impl From<EventKind> for EventKindDto {
    fn from(k: EventKind) -> EventKindDto {
        match k {
            EventKind::Created => EventKindDto::Created,
            EventKind::Edited => EventKindDto::Edited,
            EventKind::Moved => EventKindDto::Moved,
            EventKind::Status => EventKindDto::Status,
            EventKind::Note => EventKindDto::Note,
            EventKind::Linked => EventKindDto::Linked,
            EventKind::Unlinked => EventKindDto::Unlinked,
            EventKind::LinkState => EventKindDto::LinkState,
            EventKind::WaitsOnAdded => EventKindDto::WaitsOnAdded,
            EventKind::WaitsOnRemoved => EventKindDto::WaitsOnRemoved,
            EventKind::Rejected => EventKindDto::Rejected,
            EventKind::RepoLinked => EventKindDto::RepoLinked,
            EventKind::RepoUnlinked => EventKindDto::RepoUnlinked,
        }
    }
}

/// What a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum LinkKindDto {
    T3Thread,
    ClaudeSession,
    Pr,
    Slack,
    Doc,
    GithubIssue,
}

impl From<LinkKind> for LinkKindDto {
    fn from(k: LinkKind) -> LinkKindDto {
        match k {
            LinkKind::T3Thread => LinkKindDto::T3Thread,
            LinkKind::ClaudeSession => LinkKindDto::ClaudeSession,
            LinkKind::Pr => LinkKindDto::Pr,
            LinkKind::Slack => LinkKindDto::Slack,
            LinkKind::Doc => LinkKindDto::Doc,
            LinkKind::GithubIssue => LinkKindDto::GithubIssue,
        }
    }
}

/// Where a PR stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PrStateDto {
    Open,
    Merged,
    Closed,
}

impl From<PrState> for PrStateDto {
    fn from(s: PrState) -> PrStateDto {
        match s {
            PrState::Open => PrStateDto::Open,
            PrState::Merged => PrStateDto::Merged,
            PrState::Closed => PrStateDto::Closed,
        }
    }
}

/// A thread's live chip on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChipDto {
    Working,
    WaitingOnYou,
    Stopped,
}

impl From<Chip> for ChipDto {
    fn from(c: Chip) -> ChipDto {
        match c {
            Chip::Working => ChipDto::Working,
            Chip::WaitingOnYou => ChipDto::WaitingOnYou,
            Chip::Stopped => ChipDto::Stopped,
        }
    }
}

impl ChipDto {
    /// Higher is stronger: Waiting on you, then Working, then Stopped.
    pub fn strength(self) -> u8 {
        match self {
            ChipDto::WaitingOnYou => 3,
            ChipDto::Working => 2,
            ChipDto::Stopped => 1,
        }
    }
}

// ---------------------------------------------------------------------------- tasks and rows

/// A registered repo, with its fixed colour (`repo-1` … `repo-4`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RepoDto {
    #[ts(type = "number")]
    pub id: RepoId,
    pub name: String,
    pub path: String,
    pub remote_url: Option<String>,
    /// 1–4: `repo-((id − 1) mod 4) + 1`. A repo keeps its colour for ever.
    pub color: u8,
}

impl From<&Repo> for RepoDto {
    fn from(r: &Repo) -> RepoDto {
        RepoDto {
            id: r.id,
            name: r.name.clone(),
            path: r.path.clone(),
            remote_url: r.remote_url.clone(),
            color: repo_color(r.id),
        }
    }
}

/// The Owned Colour Rule: fixed by id, so a fifth repo shares repo-1's colour.
pub fn repo_color(id: RepoId) -> u8 {
    // rem_euclid keeps it in 0..4 even for a nonsense id.
    ((id.0 - 1).rem_euclid(4) + 1) as u8
}

/// A task as `core` stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskDto {
    #[ts(type = "number")]
    pub id: TaskId,
    #[ts(type = "number | null")]
    pub parent: Option<TaskId>,
    #[ts(type = "number[]")]
    pub repos: Vec<RepoId>,
    pub title: String,
    pub body: Option<String>,
    pub status: StatusDto,
    pub blocked_reason: Option<String>,
    pub position: u32,
    pub origin: OriginDto,
    pub plan_key: Option<String>,
    pub branch: Option<String>,
    pub created_at: Timestamp,
}

impl From<&Task> for TaskDto {
    fn from(t: &Task) -> TaskDto {
        TaskDto {
            id: t.id,
            parent: t.parent,
            repos: t.repos.clone(),
            title: t.title.clone(),
            body: t.body.clone(),
            status: t.status.into(),
            blocked_reason: t.blocked_reason.clone(),
            position: t.position,
            origin: t.origin.into(),
            plan_key: t.plan_key.clone(),
            branch: t.branch.clone(),
            created_at: t.created_at.clone(),
        }
    }
}

/// Leaf subtasks at any depth: dropped ones left out, proposed ones counted as todo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
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

/// One row of the task list: everything it draws, worked out in Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RowDto {
    #[ts(type = "number")]
    pub id: TaskId,
    #[ts(type = "number | null")]
    pub parent: Option<TaskId>,
    pub depth: u32,
    pub position: u32,
    pub title: String,
    pub status: StatusDto,
    pub origin: OriginDto,
    pub blocked_reason: Option<String>,
    /// Something it waits on is still open.
    pub waiting: bool,
    /// The open tasks it waits on (empty unless `waiting`).
    #[ts(type = "number[]")]
    pub waits_on: Vec<TaskId>,
    pub has_children: bool,
    /// Over leaf subtasks at any depth; `null` for a leaf.
    pub progress: Option<Progress>,
    /// Its own repos, or for a parent with none the union over its subtree.
    #[ts(type = "number[]")]
    pub repos: Vec<RepoId>,
    pub repos_from_subtasks: bool,
    /// Its branch, or the nearest ancestor's (display only).
    pub branch: Option<String>,
    /// In `core`'s ready list, and a leaf.
    pub ready: bool,
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
    pub live: Option<ChipDto>,
    /// Its newest event, or for a parent the newest anywhere in its subtree.
    pub last_at: Timestamp,
    pub last_actor: ActorKind,
}

/// What changed in the rows since the last patch. Apply it only if `seq` is greater than the
/// last one applied: a write's returned patch and the same patch as an event apply once.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct Patch {
    #[ts(type = "number")]
    pub seq: u64,
    /// Whole rows, new or changed.
    pub upserts: Vec<RowDto>,
    #[ts(type = "number[]")]
    pub removed: Vec<TaskId>,
    /// Every id in tree order, sent only when it changed.
    #[ts(type = "number[] | null")]
    pub order: Option<Vec<TaskId>>,
    /// Every repo, sent only when they changed.
    pub repos: Option<Vec<RepoDto>>,
    /// New tasks, for the new-row highlight.
    #[ts(type = "number[]")]
    pub created: Vec<TaskId>,
}

impl Patch {
    /// A patch that changes nothing, at the current `seq` (the UI skips it).
    pub fn empty(seq: u64) -> Patch {
        Patch {
            seq,
            ..Patch::default()
        }
    }

    /// Nothing in it.
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty()
            && self.removed.is_empty()
            && self.order.is_none()
            && self.repos.is_none()
    }
}

// ---------------------------------------------------------------------------- history and links

/// One event in the activity feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventDto {
    #[ts(type = "number")]
    pub id: EventId,
    #[ts(type = "number")]
    pub task: TaskId,
    pub kind: EventKindDto,
    pub to_status: Option<StatusDto>,
    pub body: Option<String>,
    pub actor: ActorDto,
    pub at: Timestamp,
}

impl EventDto {
    /// The event, with its actor named for the feed.
    pub fn new(e: &Event, actor: ActorDto) -> EventDto {
        EventDto {
            id: e.id,
            task: e.task,
            kind: e.kind.into(),
            to_status: e.to_status.map(Into::into),
            body: e.body.clone(),
            actor,
            at: e.at.clone(),
        }
    }
}

/// Where a thread runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SourceDto {
    T3,
    Claude,
}

/// Names one thread; the UI sends it back to `open_thread`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ThreadKeyDto {
    pub source: SourceDto,
    pub r#ref: String,
}

impl From<&ThreadKey> for ThreadKeyDto {
    fn from(k: &ThreadKey) -> ThreadKeyDto {
        ThreadKeyDto {
            source: match k.source {
                Source::T3 => SourceDto::T3,
                Source::Claude => SourceDto::Claude,
            },
            r#ref: k.r#ref.clone(),
        }
    }
}

impl From<&ThreadKeyDto> for ThreadKey {
    fn from(k: &ThreadKeyDto) -> ThreadKey {
        ThreadKey {
            source: match k.source {
                SourceDto::T3 => Source::T3,
                SourceDto::Claude => Source::Claude,
            },
            r#ref: k.r#ref.clone(),
        }
    }
}

/// Where a thread stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStateDto {
    Running,
    NeedsMe,
    Errored,
    Idle,
    Settled,
    Gone,
}

impl From<ThreadState> for ThreadStateDto {
    fn from(s: ThreadState) -> ThreadStateDto {
        match s {
            ThreadState::Running => ThreadStateDto::Running,
            ThreadState::NeedsMe => ThreadStateDto::NeedsMe,
            ThreadState::Errored => ThreadStateDto::Errored,
            ThreadState::Idle => ThreadStateDto::Idle,
            ThreadState::Settled => ThreadStateDto::Settled,
            ThreadState::Gone => ThreadStateDto::Gone,
        }
    }
}

/// A question or approval an agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RequestDto {
    pub kind: RequestKindDto,
    pub text: String,
    pub at: Timestamp,
}

/// What kind of answer a request wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RequestKindDto {
    Question,
    Approval,
}

impl From<&Request> for RequestDto {
    fn from(r: &Request) -> RequestDto {
        RequestDto {
            kind: match r.kind {
                RequestKind::Question => RequestKindDto::Question,
                RequestKind::Approval => RequestKindDto::Approval,
            },
            text: r.text.clone(),
            at: r.at.clone(),
        }
    }
}

/// One thread linked to a task, as `sync` last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadDto {
    pub key: ThreadKeyDto,
    pub state: ThreadStateDto,
    /// Running, Waiting or Stopped; none for a gone thread.
    pub chip: Option<ChipDto>,
    pub title: Option<String>,
    pub request: Option<RequestDto>,
    pub error: Option<String>,
    pub project_path: Option<String>,
    pub last_activity_at: Option<Timestamp>,
    pub stale: bool,
    /// `t3code://threads/…`; `null` for a Claude Code session, so "Open ↗" doesn't show.
    pub url: Option<String>,
}

impl ThreadDto {
    /// The thread, with the URL `sync.thread_url` gave for it.
    pub fn new(t: &ThreadView, url: Option<String>) -> ThreadDto {
        ThreadDto {
            key: (&t.key).into(),
            state: t.state.into(),
            chip: t.state.chip().map(Into::into),
            title: t.title.clone(),
            request: t.request.as_ref().map(Into::into),
            error: t.error.clone(),
            project_path: t.project_path.clone(),
            last_activity_at: t.last_activity_at.clone(),
            stale: t.stale,
            url,
        }
    }
}

/// GitHub's review decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDto {
    Approved,
    ChangesRequested,
    Required,
    None,
}

/// The latest commit's checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChecksDto {
    Passing,
    Failing,
    Pending,
    None,
}

/// One PR as GitHub last reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrDto {
    pub r#ref: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub state: Option<PrStateDto>,
    pub draft: bool,
    pub review: ReviewDto,
    pub checks: ChecksDto,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub stale: bool,
}

impl From<&PrView> for PrDto {
    fn from(p: &PrView) -> PrDto {
        PrDto {
            r#ref: p.r#ref.to_string(),
            url: p.url.clone(),
            title: p.title.clone(),
            state: p.state.map(Into::into),
            draft: p.draft,
            review: match p.review {
                Review::Approved => ReviewDto::Approved,
                Review::ChangesRequested => ReviewDto::ChangesRequested,
                Review::Required => ReviewDto::Required,
                Review::None => ReviewDto::None,
            },
            checks: match p.checks {
                Checks::Passing => ChecksDto::Passing,
                Checks::Failing => ChecksDto::Failing,
                Checks::Pending => ChecksDto::Pending,
                Checks::None => ChecksDto::None,
            },
            additions: p.additions,
            deletions: p.deletions,
            stale: p.stale,
        }
    }
}

/// One link on a task, with what `sync` knows about its PR or thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LinkDto {
    #[ts(type = "number")]
    pub id: LinkId,
    #[ts(type = "number")]
    pub task: TaskId,
    pub kind: LinkKindDto,
    pub r#ref: String,
    pub url: Option<String>,
    pub pr_state: Option<PrStateDto>,
    pub title: Option<String>,
    pub linked_by: ActorKind,
    pub linked_at: Timestamp,
    pub pr: Option<PrDto>,
    pub thread: Option<ThreadDto>,
}

impl LinkDto {
    /// The link, with its PR or thread view if `sync` has one.
    pub fn new(l: &Link, pr: Option<PrDto>, thread: Option<ThreadDto>) -> LinkDto {
        LinkDto {
            id: l.id,
            task: l.task,
            kind: l.kind.into(),
            r#ref: l.r#ref.clone(),
            url: l.url.clone(),
            pr_state: l.pr_state.map(Into::into),
            title: l.title.clone(),
            linked_by: (&l.linked_by).into(),
            linked_at: l.linked_at.clone(),
            pr,
            thread,
        }
    }
}

/// Another task in a Waits on or Blocks list, with its status icon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DepDto {
    #[ts(type = "number")]
    pub id: TaskId,
    pub title: String,
    pub status: StatusDto,
}

/// One step of the breadcrumb ("Booking flow › 144").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CrumbDto {
    #[ts(type = "number")]
    pub id: TaskId,
    pub title: String,
}

// ---------------------------------------------------------------------------- needs me

/// What a Needs me item is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NeedsKindDto {
    Question,
    Approval,
    Review,
    FailedRun,
    Proposed,
    Ready,
    FinishIt,
    Quiet,
}

impl From<NeedsKind> for NeedsKindDto {
    fn from(k: NeedsKind) -> NeedsKindDto {
        match k {
            NeedsKind::Question => NeedsKindDto::Question,
            NeedsKind::Approval => NeedsKindDto::Approval,
            NeedsKind::Review => NeedsKindDto::Review,
            NeedsKind::FailedRun => NeedsKindDto::FailedRun,
            NeedsKind::Proposed => NeedsKindDto::Proposed,
            NeedsKind::Ready => NeedsKindDto::Ready,
            NeedsKind::FinishIt => NeedsKindDto::FinishIt,
            NeedsKind::Quiet => NeedsKindDto::Quiet,
        }
    }
}

/// The design's seven groups, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NeedsGroupDto {
    Questions,
    Reviews,
    FailedRuns,
    Proposed,
    Ready,
    FinishIt,
    Quiet,
}

impl From<NeedsGroup> for NeedsGroupDto {
    fn from(g: NeedsGroup) -> NeedsGroupDto {
        match g {
            NeedsGroup::Questions => NeedsGroupDto::Questions,
            NeedsGroup::Reviews => NeedsGroupDto::Reviews,
            NeedsGroup::FailedRuns => NeedsGroupDto::FailedRuns,
            NeedsGroup::Proposed => NeedsGroupDto::Proposed,
            NeedsGroup::Ready => NeedsGroupDto::Ready,
            NeedsGroup::FinishIt => NeedsGroupDto::FinishIt,
            NeedsGroup::Quiet => NeedsGroupDto::Quiet,
        }
    }
}

/// What an item's main action opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetDto {
    Thread {
        key: ThreadKeyDto,
    },
    Pr {
        r#ref: String,
        url: Option<String>,
    },
    Task {
        #[ts(type = "number")]
        id: TaskId,
    },
}

impl From<&Target> for TargetDto {
    fn from(t: &Target) -> TargetDto {
        match t {
            Target::Thread(k) => TargetDto::Thread { key: k.into() },
            Target::Pr { r#ref, url } => TargetDto::Pr {
                r#ref: r#ref.to_string(),
                url: url.clone(),
            },
            Target::Task(id) => TargetDto::Task { id: *id },
        }
    }
}

/// One proposal on a Proposed item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProposalDto {
    #[ts(type = "number")]
    pub id: TaskId,
    pub title: String,
}

/// One thing waiting on me.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NeedsItemDto {
    /// Stable across restarts; the UI sends it back to dismiss, snooze and restore.
    pub key: String,
    pub kind: NeedsKindDto,
    pub group: NeedsGroupDto,
    #[ts(type = "number")]
    pub task: TaskId,
    pub task_title: String,
    pub since: Timestamp,
    pub headline: String,
    pub context: String,
    pub target: TargetDto,
    pub proposed: Vec<ProposalDto>,
    pub stale: bool,
}

impl From<&NeedsItem> for NeedsItemDto {
    fn from(i: &NeedsItem) -> NeedsItemDto {
        NeedsItemDto {
            key: i.key.0.clone(),
            kind: i.kind.into(),
            group: i.group.into(),
            task: i.task,
            task_title: i.task_title.clone(),
            since: i.since.clone(),
            headline: i.headline.clone(),
            context: i.context.clone(),
            target: (&i.target).into(),
            proposed: i
                .proposed
                .iter()
                .map(|(id, title)| ProposalDto {
                    id: *id,
                    title: title.clone(),
                })
                .collect(),
            stale: i.stale,
        }
    }
}

/// The Needs me list: `bootstrap`'s and `needs:changed`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct NeedsDto {
    #[ts(type = "number")]
    pub version: u64,
    pub items: Vec<NeedsItemDto>,
    pub count: u32,
    pub checked_at: Option<Timestamp>,
}

impl From<&NeedsList> for NeedsDto {
    fn from(l: &NeedsList) -> NeedsDto {
        NeedsDto {
            version: l.version,
            items: l.items.iter().map(Into::into).collect(),
            count: l.count,
            checked_at: l.checked_at.clone(),
        }
    }
}

// ---------------------------------------------------------------------------- connections

/// One line of the connections popover. Rows with the same name are merged into one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConnectionDto {
    pub name: String,
    pub ok: bool,
    pub state: String,
    pub detail: String,
    pub fix: Option<String>,
    /// For "Agents": when an agent last reported.
    pub last_at: Option<Timestamp>,
}

impl From<&Connection> for ConnectionDto {
    fn from(c: &Connection) -> ConnectionDto {
        ConnectionDto {
            name: c.name.clone(),
            ok: c.ok,
            state: c.state.clone(),
            detail: c.detail.clone(),
            fix: c.fix.clone(),
            last_at: None,
        }
    }
}

/// Whether T3 Code can start threads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AvailabilityDto {
    Unknown,
    Ready { version: String },
    Missing { reason: String },
}

impl From<&Availability> for AvailabilityDto {
    fn from(a: &Availability) -> AvailabilityDto {
        match a {
            Availability::Unknown => AvailabilityDto::Unknown,
            Availability::Ready { version } => AvailabilityDto::Ready {
                version: version.clone(),
            },
            Availability::Missing { reason } => AvailabilityDto::Missing {
                reason: reason.clone(),
            },
        }
    }
}

/// `connections:changed`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConnectionsDto {
    pub connections: Vec<ConnectionDto>,
    pub availability: AvailabilityDto,
}

// ---------------------------------------------------------------------------- launch

/// Do it or Plan it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ModeDto {
    DoIt,
    PlanIt,
}

impl From<Mode> for ModeDto {
    fn from(m: Mode) -> ModeDto {
        match m {
            Mode::DoIt => ModeDto::DoIt,
            Mode::PlanIt => ModeDto::PlanIt,
        }
    }
}

impl From<ModeDto> for Mode {
    fn from(m: ModeDto) -> Mode {
        match m {
            ModeDto::DoIt => Mode::DoIt,
            ModeDto::PlanIt => Mode::PlanIt,
        }
    }
}

/// One thread `launch` started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StartedThreadDto {
    pub key: ThreadKeyDto,
    pub mode: ModeDto,
    #[ts(type = "number | null")]
    pub repo: Option<RepoId>,
    pub title: String,
    pub linked: bool,
}

impl From<&StartedThread> for StartedThreadDto {
    fn from(s: &StartedThread) -> StartedThreadDto {
        StartedThreadDto {
            key: (&s.key).into(),
            mode: s.mode.into(),
            repo: s.repo,
            title: s.title.clone(),
            linked: s.linked,
        }
    }
}

/// A service's error, as it came.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ServiceErrorDto {
    pub code: crate::errors::ErrorCode,
    pub message: String,
}

impl From<&ServiceError> for ServiceErrorDto {
    fn from(e: &ServiceError) -> ServiceErrorDto {
        ServiceErrorDto {
            code: e.code.into(),
            message: e.message.clone(),
        }
    }
}

/// A place that failed to start: a repo, or `null` for the research folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FailedPlaceDto {
    #[ts(type = "number | null")]
    pub repo: Option<RepoId>,
    pub error: ServiceErrorDto,
}

/// What one Do it or Plan it did, place by place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LaunchReportDto {
    pub started: Vec<StartedThreadDto>,
    #[ts(type = "number[]")]
    pub skipped: Vec<RepoId>,
    pub failed: Vec<FailedPlaceDto>,
    pub branch: Option<String>,
    pub moved_to_doing: bool,
}

impl From<&LaunchReport> for LaunchReportDto {
    fn from(r: &LaunchReport) -> LaunchReportDto {
        LaunchReportDto {
            started: r.started.iter().map(Into::into).collect(),
            skipped: r.skipped.clone(),
            failed: r
                .failed
                .iter()
                .map(|(repo, error)| FailedPlaceDto {
                    repo: *repo,
                    error: error.into(),
                })
                .collect(),
            branch: r.branch.clone(),
            moved_to_doing: r.moved_to_doing,
        }
    }
}

/// What Do it would do, for the panel and the Start in menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PreparedDto {
    pub repos: Vec<RepoDto>,
    pub from_subtasks: bool,
    pub needs_choice: bool,
    pub choices: Vec<RepoDto>,
    /// The branch Do it would use (shown in text-3 until it's saved).
    pub branch: Option<String>,
    pub research_dir: String,
    #[ts(type = "number[]")]
    pub waiting_on: Vec<TaskId>,
    pub started: Vec<StartedThreadDto>,
}

impl From<&Prepared> for PreparedDto {
    fn from(p: &Prepared) -> PreparedDto {
        PreparedDto {
            repos: p.repos.iter().map(Into::into).collect(),
            from_subtasks: p.from_subtasks,
            needs_choice: p.needs_choice,
            choices: p.choices.iter().map(Into::into).collect(),
            branch: p.branch.clone(),
            research_dir: path_text(&p.research_dir),
            waiting_on: p.waiting_on.clone(),
            started: p.started.iter().map(Into::into).collect(),
        }
    }
}

/// A path as text for the UI.
pub fn path_text(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

/// What a toast's button runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToastCommand {
    /// `open_thread(key)`.
    OpenThread { key: ThreadKeyDto },
    /// The same `do_it` or `plan_it` again; `launch` skips what already started.
    Retry {
        #[ts(type = "number")]
        task: TaskId,
        mode: ModeDto,
    },
    /// `add_to_t3(repo)`, then the same request again.
    AddToT3AndRetry {
        #[ts(type = "number")]
        repo: RepoId,
        #[ts(type = "number")]
        task: TaskId,
        mode: ModeDto,
    },
    /// The same request with `again: true`.
    StartAgain {
        #[ts(type = "number")]
        task: TaskId,
        mode: ModeDto,
    },
    /// `reopen_and_start(task)`.
    ReopenAndStart {
        #[ts(type = "number")]
        task: TaskId,
    },
    /// `undo(entry)`.
    Undo {
        #[ts(type = "number")]
        entry: u64,
    },
}

/// One button on a toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToastAction {
    pub label: String,
    pub command: ToastCommand,
}

/// A toast's tone: plain, or with the orange warning icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ToastTone {
    Info,
    Warning,
}

/// A toast, built in Rust: Do it and Plan it results, and the `toast` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToastDto {
    pub text: String,
    pub tone: ToastTone,
    pub actions: Vec<ToastAction>,
    pub undo: Option<UndoInfo>,
    /// The task it's about, so the UI can select it.
    #[ts(type = "number | null")]
    pub task: Option<TaskId>,
}

/// An undo entry the UI can name and undo from a toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UndoInfo {
    #[ts(type = "number")]
    pub entry: u64,
    /// e.g. "Mark done 144".
    pub label: String,
}

// ---------------------------------------------------------------------------- reads

/// The display waves of a parent's subtasks (done ones included), for the Waves list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct WavesDto {
    /// Each wave's subtask ids, in sibling order.
    #[ts(type = "number[][]")]
    pub waves: Vec<Vec<TaskId>>,
    /// Subtasks held by open tasks outside the parent, with those tasks.
    pub outside: Vec<OutsideDto>,
    /// Subtasks that wait on each other in a loop.
    #[ts(type = "number[]")]
    pub tangled: Vec<TaskId>,
}

/// A subtask waiting on open tasks outside its parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OutsideDto {
    #[ts(type = "number")]
    pub task: TaskId,
    #[ts(type = "number[]")]
    pub blockers: Vec<TaskId>,
}

/// Everything the task panel and page show for one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskDetail {
    /// The request number this answers; the UI ignores a stale one.
    pub req: u32,
    pub task: TaskDto,
    /// From the top-level task down to the parent.
    pub ancestors: Vec<CrumbDto>,
    pub links: Vec<LinkDto>,
    pub threads: Vec<ThreadDto>,
    pub prs: Vec<PrDto>,
    /// Newest first, from the task and everything under it.
    pub history: Vec<EventDto>,
    /// The history was cut at the limit; "Show more" asks for 200.
    pub history_more: bool,
    pub waits_on: Vec<DepDto>,
    pub blocks: Vec<DepDto>,
    /// For a task with subtasks.
    pub waves: Option<WavesDto>,
    pub needs: Vec<NeedsItemDto>,
    /// What Do it would do; `null` for proposed, done and dropped tasks.
    pub prepared: Option<PreparedDto>,
    /// Why `prepare` failed, e.g. `t3_missing`.
    pub prepare_error: Option<AppError>,
    /// My open PRs from this task's branch, to link with one click.
    pub suggested_prs: Vec<PrDto>,
}

/// One station on the run-order map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunNodeDto {
    #[ts(type = "number")]
    pub id: TaskId,
    /// 0-based display wave.
    pub wave: u32,
    /// 0-based place within the wave.
    pub index: u32,
}

/// A line from a blocker to the task waiting on it, lifted to the parent's subtasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunEdgeDto {
    #[ts(type = "number")]
    pub from: TaskId,
    #[ts(type = "number")]
    pub to: TaskId,
    /// Every blocker behind it is done or dropped: a solid line.
    pub done: bool,
}

/// The run-order map of a parent's direct subtasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunMap {
    pub req: u32,
    #[ts(type = "number")]
    pub parent: TaskId,
    pub nodes: Vec<RunNodeDto>,
    pub edges: Vec<RunEdgeDto>,
    #[ts(type = "number[][]")]
    pub waves: Vec<Vec<TaskId>>,
    pub outside: Vec<OutsideDto>,
    #[ts(type = "number[]")]
    pub tangled: Vec<TaskId>,
    /// At most 40 subtasks and 8 waves; otherwise the page opens on Waves.
    pub fits: bool,
}

/// One dot per wave on a plan card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WaveDot {
    Done,
    Current,
    Todo,
}

/// What a plan card says is next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NextUp {
    /// Up to two ready subtasks.
    Ready {
        #[ts(type = "number[]")]
        ids: Vec<TaskId>,
    },
    /// Every subtask is done: "Finish it?".
    FinishIt,
    /// "Nothing ready".
    Nothing,
}

/// One card on Plans: an open top-level parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlanCard {
    #[ts(type = "number")]
    pub id: TaskId,
    pub title: String,
    pub status: StatusDto,
    pub progress: Progress,
    pub percent: u32,
    pub wave_count: u32,
    /// 0-based: the lowest wave with an open task; `null` when every wave is done.
    pub current_wave: Option<u32>,
    pub wave_dots: Vec<WaveDot>,
    pub next_up: NextUp,
    /// Subtasks with a working agent.
    pub agents_working: u32,
    pub blocked: u32,
    #[ts(type = "number[]")]
    pub repos: Vec<RepoId>,
    pub last_at: Timestamp,
}

/// `plans`' answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlansDto {
    pub req: u32,
    pub cards: Vec<PlanCard>,
}

/// An open task quick add can put a new task under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ParentOption {
    #[ts(type = "number")]
    pub id: TaskId,
    pub title: String,
    pub depth: u32,
}

/// What quick add needs: the repos and the open tasks, in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuickAddData {
    pub repos: Vec<RepoDto>,
    pub parents: Vec<ParentOption>,
    /// The hotkey as the header shows it, e.g. "⌃⌥N".
    pub hotkey: String,
}

/// One of T3's projects, for Add repos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct T3ProjectDto {
    pub path: String,
    pub name: String,
    /// Has a `.git`; a folder that isn't is shown greyed.
    pub is_git: bool,
    /// Already a repo here.
    pub added: bool,
}

/// Everything the main window draws first, read once before it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Bootstrap {
    #[ts(type = "number")]
    pub seq: u64,
    /// In tree order.
    pub rows: Vec<RowDto>,
    #[ts(type = "number[]")]
    pub order: Vec<TaskId>,
    pub repos: Vec<RepoDto>,
    pub needs: NeedsDto,
    pub connections: Vec<ConnectionDto>,
    pub availability: AvailabilityDto,
    pub settings: Settings,
    /// The system accent as `#rrggbb`, or `null` for Multicolor (keep indigo).
    pub accent: Option<String>,
    /// No tasks yet: All tasks shows the first-run screen.
    pub first_run: bool,
    /// The hotkey as shown, e.g. "⌃⌥N".
    pub hotkey: String,
    pub db_path: Option<String>,
}

// ---------------------------------------------------------------------------- write results

/// A write that may be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Undoable {
    pub patch: Patch,
    /// `null` when the write changed nothing.
    pub undo: Option<UndoInfo>,
}

/// What Do it or Plan it did. Returned once per press, so its size doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchOutcome {
    /// The task has no repo: ask where (the Start in menu).
    NeedsChoice {
        choices: Vec<RepoDto>,
        research_dir: String,
    },
    /// `start` ran; the toast says what happened.
    Started {
        report: LaunchReportDto,
        patch: Patch,
        toast: ToastDto,
    },
}

/// A Do it or Plan it that followed a create: what it did, or why it failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LaunchAttempt {
    pub outcome: Option<LaunchOutcome>,
    pub error: Option<AppError>,
}

/// `create_task`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Created {
    #[ts(type = "number")]
    pub id: TaskId,
    pub patch: Patch,
    /// When `then` asked for Do it or Plan it.
    pub launch: Option<LaunchAttempt>,
    /// Links that couldn't be added, by title: "Created 153, but couldn't link <title>".
    pub failed_links: Vec<String>,
}

/// A folder that couldn't be added as a repo, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FailedPath {
    pub path: String,
    pub message: String,
}

/// `add_repos`' answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AddedRepos {
    /// The repos just added.
    pub repos: Vec<RepoDto>,
    pub patch: Patch,
    pub failed: Vec<FailedPath>,
}

/// `undo`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UndoResult {
    pub patch: Option<Patch>,
    pub message: String,
}

/// `install_cli`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InstalledCli {
    pub path: String,
    /// `~/.local/bin` is on the login shell's `PATH`.
    pub on_path: bool,
}

// ---------------------------------------------------------------------------- events

/// `menu:action`'s payload: a native menu item was chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MenuAction {
    pub command: String,
}

/// `nav:open`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct NavOpen {
    pub view: Option<String>,
    #[ts(type = "number | null")]
    pub task: Option<TaskId>,
    pub needs_key: Option<String>,
}

/// `quickadd:reset`'s payload: the panel hid; reset the form and refetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuickAddReset {
    /// The hotkey as shown, e.g. "⌃⌥N".
    pub hotkey: String,
    /// Hidden by a click elsewhere: keep a non-empty title and notes.
    pub keep_text: bool,
}

/// The theme the windows follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThemeDto {
    Dark,
    Light,
}

/// `appearance:changed`'s payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppearanceDto {
    pub accent: Option<String>,
    pub theme: ThemeDto,
}

/// `error:background`'s payload: the watcher or an app thread failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackgroundError {
    pub source: String,
    pub message: String,
}

/// A research folder or a picked folder, as text.
pub fn folder_text(p: Option<PathBuf>) -> Option<String> {
    p.map(|p| path_text(&p))
}
