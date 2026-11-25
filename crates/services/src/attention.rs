//! What `attention` shows: the Needs me list, and the trait the app calls.

use std::sync::Arc;

use airboss_core::TaskId;
use serde::{Deserialize, Serialize};

use crate::{PrRef, Service, ServiceError, ThreadKey, Timestamp};

/// The Needs me list and its menu-bar count.
pub trait AttentionService: Service {
    /// The current list; no I/O.
    fn needs_me(&self) -> Arc<NeedsList>;
    /// The items about one task, in list order, for its panel's callouts.
    fn for_task(&self, task: TaskId) -> Vec<NeedsItem>;
    /// Hides this occurrence for good. A new occurrence has a new key, so it shows.
    fn dismiss(&self, k: &NeedsKey) -> Result<(), ServiceError>;
    /// Hides it until `until`; `None` means the default 3 days.
    fn snooze(&self, k: &NeedsKey, until: Option<Timestamp>) -> Result<(), ServiceError>;
    /// Undoes a dismiss or snooze. `NotFound` if the key isn't hidden.
    fn restore(&self, k: &NeedsKey) -> Result<(), ServiceError>;
}

/// Everything waiting on me, in display order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsList {
    pub items: Vec<NeedsItem>,
    /// `items.len()`: the menu-bar count.
    pub count: u32,
    /// When sync last read every source that's ok.
    pub checked_at: Option<Timestamp>,
    /// Goes up by one each time the visible list changes.
    pub version: u64,
}

/// Names one occurrence, e.g. `"question:<request id>"`; stable across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NeedsKey(pub String);

/// One thing waiting on me.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsItem {
    pub key: NeedsKey,
    pub kind: NeedsKind,
    pub group: NeedsGroup,
    pub task: TaskId,
    pub task_title: String,
    /// When it started waiting: the request, the PR becoming ready, the last activity …
    pub since: Timestamp,
    pub headline: String,
    pub context: String,
    /// What the main action opens.
    pub target: Target,
    /// The proposals, for a Proposed item; empty otherwise.
    pub proposed: Vec<(TaskId, String)>,
    /// Built from a thread or PR whose source is down, so it may be out of date.
    pub stale: bool,
}

/// What an item is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedsKind {
    Question,
    Approval,
    Review,
    FailedRun,
    Proposed,
    Ready,
    FinishIt,
    Quiet,
}

impl NeedsKind {
    /// The design's group for this kind: questions and approvals share one.
    pub fn group(self) -> NeedsGroup {
        match self {
            NeedsKind::Question | NeedsKind::Approval => NeedsGroup::Questions,
            NeedsKind::Review => NeedsGroup::Reviews,
            NeedsKind::FailedRun => NeedsGroup::FailedRuns,
            NeedsKind::Proposed => NeedsGroup::Proposed,
            NeedsKind::Ready => NeedsGroup::Ready,
            NeedsKind::FinishIt => NeedsGroup::FinishIt,
            NeedsKind::Quiet => NeedsGroup::Quiet,
        }
    }
}

/// The design's seven groups, declared in display order, so sorting by group is display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedsGroup {
    Questions,
    Reviews,
    FailedRuns,
    Proposed,
    Ready,
    FinishIt,
    Quiet,
}

/// What an item's main action opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Thread(ThreadKey),
    Pr { r#ref: PrRef, url: Option<String> },
    Task(TaskId),
}
