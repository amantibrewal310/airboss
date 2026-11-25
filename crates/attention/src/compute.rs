//! The one pure function that decides what Needs me shows.
//!
//! Every timestamp is compared as `core`'s text (`"2026-11-25T17:19:07.841Z"`), which sorts
//! as time. Anything else (GitHub's `…:08Z`) is rewritten into that form first, and each
//! threshold becomes one cut-off string per call, so 2,000 tasks cost no date parsing.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_core::{CoreSnapshot, Status, TaskId, TaskNode};
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{
    NeedsGroup, NeedsItem, NeedsKey, NeedsKind, SyncSnapshot, Target, ThreadState, Timestamp,
};

use crate::{AttentionConfig, kinds, quiet, stopped};

/// Everything `compute` reads: the latest `core` and `sync` snapshots.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub core: Arc<CoreSnapshot>,
    pub sync: Arc<SyncSnapshot>,
}

/// One dismissed or snoozed item: hidden while `until` is `None` or later than now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    pub key: NeedsKey,
    pub until: Option<SystemTime>,
}

/// What `compute` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Computed {
    /// The visible items, in display order.
    pub items: Vec<NeedsItem>,
    /// Every item's key, hidden ones included, in no particular order.
    pub live_keys: Vec<NeedsKey>,
    /// The next moment the visible list could change with no new input. Always later than
    /// `now`: anything already due is in `items`.
    pub next_wake: Option<SystemTime>,
}

/// Pure: everything attention shows follows from these.
pub fn compute(
    input: &Inputs,
    hidden: &[Hidden],
    now: SystemTime,
    cfg: &AttentionConfig,
) -> Computed {
    let full = compute_full(input, hidden, now, cfg);
    Computed {
        items: full.entries.into_iter().map(|e| e.item).collect(),
        live_keys: full.live.into_iter().map(|l| l.key).collect(),
        next_wake: full.next_wake,
    }
}

/// One occurrence, with what the list itself doesn't carry.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub item: NeedsItem,
    /// Every open task the item is about, lowest first (`item.task`). A thread or PR on
    /// several tasks is one item, shown as a callout on each of them.
    pub tasks: Vec<TaskId>,
    /// When this item, while visible, leaves or changes with no new input.
    pub changes_at: Option<SystemTime>,
}

/// A live item's key, with what a hidden row records about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Live {
    pub key: NeedsKey,
    pub kind: NeedsKind,
    pub task: TaskId,
}

/// `compute`, keeping what the worker needs too.
#[derive(Debug, Clone)]
pub(crate) struct Full {
    /// The visible entries, in display order.
    pub entries: Vec<Entry>,
    pub live: Vec<Live>,
    pub next_wake: Option<SystemTime>,
}

pub(crate) fn compute_full(
    input: &Inputs,
    hidden: &[Hidden],
    now: SystemTime,
    cfg: &AttentionConfig,
) -> Full {
    let ctx = Ctx::new(&input.core, &input.sync, now, cfg);
    let mut wake = Wake::new(now);
    let mut all = Vec::new();
    kinds::threads(&ctx, &mut all);
    stopped::stopped(&ctx, &mut all, &mut wake);
    kinds::reviews(&ctx, &mut all);
    kinds::proposed(&ctx, &mut all);
    kinds::ready(&ctx, &mut all);
    kinds::finish(&ctx, &mut all);
    quiet::quiet(&ctx, &mut all, &mut wake);

    let live = all
        .iter()
        .map(|e| Live {
            key: e.item.key.clone(),
            kind: e.item.kind,
            task: e.item.task,
        })
        .collect();
    let until: HashMap<&NeedsKey, Option<SystemTime>> =
        hidden.iter().map(|h| (&h.key, h.until)).collect();
    let mut entries: Vec<Entry> = all
        .into_iter()
        .filter(|e| match until.get(&e.item.key) {
            None => true,
            Some(None) => false,
            Some(Some(end)) if *end > now => {
                wake.at(*end);
                false
            }
            Some(Some(_)) => true,
        })
        .collect();
    // Quiet gives way to a visible Agent stopped item on the same task: one reminder at a
    // time. Dismissed, Agent stopped leaves the longer reminder to Quiet.
    let stopped_on: HashSet<TaskId> = entries
        .iter()
        .filter(|e| e.item.kind == NeedsKind::AgentStopped)
        .map(|e| e.item.task)
        .collect();
    if !stopped_on.is_empty() {
        entries.retain(|e| !(e.item.kind == NeedsKind::Quiet && stopped_on.contains(&e.item.task)));
    }
    for at in entries.iter().filter_map(|e| e.changes_at) {
        wake.at(at);
    }
    entries.sort_by(|a, b| order(&a.item, &b.item));
    let next_wake = wake.0;
    debug_assert!(next_wake.is_none_or(|t| t > now));
    Full {
        entries,
        live,
        next_wake,
    }
}

/// The design's order: by group; inside one, newest `since` first, but longest quiet first;
/// ties by task id, then key.
pub(crate) fn order(a: &NeedsItem, b: &NeedsItem) -> Ordering {
    a.group
        .cmp(&b.group)
        .then_with(|| match a.group {
            NeedsGroup::Quiet => a.since.cmp(&b.since),
            _ => b.since.cmp(&a.since),
        })
        .then_with(|| a.task.cmp(&b.task))
        .then_with(|| a.key.cmp(&b.key))
}

/// The earliest deadline later than `now`.
pub(crate) struct Wake(Option<SystemTime>, SystemTime);

impl Wake {
    fn new(now: SystemTime) -> Wake {
        Wake(None, now)
    }

    /// Keeps `t` if it's later than now and earlier than any so far.
    pub fn at(&mut self, t: SystemTime) {
        if t > self.1 && self.0.is_none_or(|w| t < w) {
            self.0 = Some(t);
        }
    }
}

/// What every kind reads, indexed once per call.
pub(crate) struct Ctx<'a> {
    pub core: &'a CoreSnapshot,
    pub sync: &'a SyncSnapshot,
    pub now: SystemTime,
    pub cfg: &'a AttentionConfig,
    /// Each task's position in `core.tree`.
    pub index: HashMap<TaskId, usize>,
    /// Tasks with a linked thread that's `Running` or `NeedsMe`: an agent is on them.
    pub agent_on: HashSet<TaskId>,
}

impl<'a> Ctx<'a> {
    pub fn new(
        core: &'a CoreSnapshot,
        sync: &'a SyncSnapshot,
        now: SystemTime,
        cfg: &'a AttentionConfig,
    ) -> Ctx<'a> {
        let index = core
            .tree
            .iter()
            .enumerate()
            .map(|(i, n)| (n.task.id, i))
            .collect();
        let agent_on = sync
            .thread_tasks
            .iter()
            .filter(|(key, _)| sync.threads.get(*key).is_some_and(|t| is_live(t.state)))
            .flat_map(|(_, tasks)| tasks.iter().copied())
            .collect();
        Ctx {
            core,
            sync,
            now,
            cfg,
            index,
            agent_on,
        }
    }

    pub fn node(&self, task: TaskId) -> Option<&'a TaskNode> {
        self.index.get(&task).map(|&i| &self.core.tree[i])
    }

    pub fn is_open(&self, task: TaskId) -> bool {
        self.node(task).is_some_and(|n| is_open(n.task.status))
    }

    /// The open ones among `tasks`, lowest first, each once.
    pub fn open_tasks(&self, tasks: &[TaskId]) -> Vec<TaskId> {
        let mut open: Vec<TaskId> = tasks.iter().copied().filter(|&t| self.is_open(t)).collect();
        open.sort_unstable();
        open.dedup();
        open
    }

    /// `now − by` as `core`'s text: a time at or before it is at least `by` old.
    pub fn cutoff(&self, by: Duration) -> Timestamp {
        to_ts(self.now.checked_sub(by).unwrap_or(UNIX_EPOCH))
    }
}

/// `todo`, `doing`, `in_review` or `blocked`.
pub(crate) fn is_open(status: Status) -> bool {
    matches!(
        status,
        Status::Todo | Status::Doing | Status::InReview | Status::Blocked
    )
}

/// An agent is working on it or waiting on me.
pub(crate) fn is_live(state: ThreadState) -> bool {
    matches!(state, ThreadState::Running | ThreadState::NeedsMe)
}

/// `ts` in `core`'s text form, borrowed when it already is; `None` if it isn't a time.
pub(crate) fn canonical(ts: &str) -> Option<Cow<'_, str>> {
    let b = ts.as_bytes();
    if b.len() == 24 && b[19] == b'.' && b[23] == b'Z' && b[10] == b'T' {
        Some(Cow::Borrowed(ts))
    } else {
        from_ts(ts).map(|t| Cow::Owned(to_ts(t)))
    }
}

/// `ts` in `core`'s form for an item's `since`; kept as it is if it can't be read.
pub(crate) fn stamp(ts: &str) -> Timestamp {
    canonical(ts).map_or_else(|| ts.to_string(), Cow::into_owned)
}

/// An item about `node`, with the fields every kind fills the same way.
pub(crate) struct ItemFor<'a> {
    pub kind: NeedsKind,
    pub key: String,
    pub node: &'a TaskNode,
    pub since: Timestamp,
    pub headline: String,
    pub context: String,
    pub target: Target,
}

impl ItemFor<'_> {
    pub fn build(self, stale: bool) -> NeedsItem {
        NeedsItem {
            key: NeedsKey(self.key),
            kind: self.kind,
            group: self.kind.group(),
            task: self.node.task.id,
            task_title: self.node.task.title.clone(),
            since: self.since,
            headline: self.headline,
            context: self.context,
            target: self.target,
            proposed: Vec::new(),
            stale,
        }
    }
}

/// `n` and the noun, singular for one: "1 subtask", "3 subtasks".
pub(crate) fn count(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        _ => format!("{n} {noun}s"),
    }
}
