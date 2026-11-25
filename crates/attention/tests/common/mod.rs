//! Hand-built inputs for `compute` and the worker: a `CoreSnapshot` builder, thread and PR
//! views, and times relative to one fixed moment.
#![allow(dead_code, unused_imports, clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use airboss_attention::{AttentionConfig, Computed, Hidden, Inputs, compute};
use airboss_core::{
    Actor, CoreSnapshot, EventId, Origin, PrState, ReadyTask, Status, Task, TaskId, TaskNode,
};
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{
    Checks, NeedsKey, PrRef, PrView, Request, RequestKind, Review, Source, SyncSnapshot, ThreadKey,
    ThreadState, ThreadView,
};

pub const DAY: Duration = Duration::from_secs(86_400);
pub const HOUR: Duration = Duration::from_secs(3_600);

/// The moment every test measures from: 1 Nov 2026, midnight UTC.
pub fn t0() -> SystemTime {
    from_ts("2026-11-01T00:00:00.000Z").unwrap()
}

/// `t0` plus `days` and `hours`.
pub fn at(days: u64, hours: u64) -> SystemTime {
    t0() + DAY * days as u32 + HOUR * hours as u32
}

/// The same, as `core`'s text.
pub fn ts(days: u64, hours: u64) -> String {
    to_ts(at(days, hours))
}

pub fn id(n: i64) -> TaskId {
    TaskId(n)
}

pub fn key(k: &str) -> NeedsKey {
    NeedsKey(k.to_string())
}

/// A `CoreSnapshot` built task by task. Add tasks depth-first (a parent before its
/// children, siblings in order), as `core`'s `tree` lists them.
#[derive(Default)]
pub struct Core {
    nodes: Vec<TaskNode>,
    edges: Vec<(TaskId, TaskId)>,
    ready: Vec<ReadyTask>,
}

impl Core {
    pub fn new() -> Core {
        Core::default()
    }

    /// A task created at `t0`, whose own newest event is at `last`.
    pub fn task(self, n: i64, parent: Option<i64>, status: Status, last: &str) -> Core {
        self.titled(n, parent, status, last, &format!("Task {n}"))
    }

    pub fn titled(
        mut self,
        n: i64,
        parent: Option<i64>,
        status: Status,
        last: &str,
        title: &str,
    ) -> Core {
        let depth = parent
            .and_then(|p| self.nodes.iter().find(|x| x.task.id.0 == p))
            .map_or(0, |p| p.depth + 1);
        let task = Task {
            id: TaskId(n),
            parent: parent.map(TaskId),
            repos: Vec::new(),
            title: title.to_string(),
            body: None,
            status,
            blocked_reason: None,
            position: 0,
            origin: Origin::Me,
            plan_key: None,
            branch: None,
            created_at: ts(0, 0),
        };
        self.nodes.push(TaskNode {
            task,
            depth,
            waiting: false,
            last_event_at: last.to_string(),
            last_actor: Actor::Me,
            subtree_last_event_at: last.to_string(),
        });
        self
    }

    /// Sets the last task's `created_at`.
    pub fn created(mut self, at: &str) -> Core {
        if let Some(n) = self.nodes.last_mut() {
            n.task.created_at = at.to_string();
        }
        self
    }

    /// `task` waits on `on`.
    pub fn edge(mut self, task: i64, on: i64) -> Core {
        self.edges.push((TaskId(task), TaskId(on)));
        self
    }

    /// Marks `task` ready since `since`. It must already be added.
    pub fn ready(mut self, task: i64, since: &str) -> Core {
        let t = self
            .nodes
            .iter()
            .find(|n| n.task.id.0 == task)
            .unwrap()
            .task
            .clone();
        self.ready.push(ReadyTask {
            task: t,
            since: since.to_string(),
        });
        self
    }

    /// The snapshot, with `subtree_last_event_at` worked out as `core` does.
    pub fn build(mut self) -> CoreSnapshot {
        let index: HashMap<TaskId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.task.id, i))
            .collect();
        for i in (0..self.nodes.len()).rev() {
            if let Some(&p) = self.nodes[i].task.parent.and_then(|p| index.get(&p)) {
                let child = self.nodes[i].subtree_last_event_at.clone();
                if child > self.nodes[p].subtree_last_event_at {
                    self.nodes[p].subtree_last_event_at = child;
                }
            }
        }
        self.edges.sort();
        CoreSnapshot {
            tree: self.nodes,
            edges: self.edges,
            ready: self.ready,
            latest_event_id: EventId(1),
        }
    }
}

pub fn t3(r: &str) -> ThreadKey {
    ThreadKey {
        source: Source::T3,
        r#ref: r.to_string(),
    }
}

/// A linked T3 thread in `state`, titled after its ref, last active at `t0`.
pub fn thread(r: &str, state: ThreadState) -> ThreadView {
    ThreadView {
        key: t3(r),
        linked: true,
        state,
        title: Some(format!("{r}-thread")),
        request: None,
        error: None,
        run_id: None,
        project_path: None,
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: Some(ts(0, 0)),
        stale: false,
    }
}

pub fn request(id: &str, kind: RequestKind, text: &str, at: &str) -> Request {
    Request {
        id: id.to_string(),
        kind,
        text: text.to_string(),
        at: at.to_string(),
    }
}

pub fn pr_ref(n: u64) -> PrRef {
    PrRef::new("acme/web", n).unwrap()
}

/// An open, ready-for-review PR `acme/web#n` titled "Change n", linked.
pub fn pr(n: u64) -> PrView {
    PrView {
        r#ref: pr_ref(n),
        linked: true,
        url: Some(format!("https://github.com/acme/web/pull/{n}")),
        title: Some(format!("Change {n}")),
        state: Some(PrState::Open),
        draft: false,
        review: Review::Required,
        checks: Checks::Passing,
        additions: Some(10),
        deletions: Some(2),
        head_ref: None,
        author: None,
        updated_at: Some(ts(0, 0)),
        ready_since: Some(ts(0, 0)),
        problem: None,
        stale: false,
    }
}

/// A `SyncSnapshot` built link by link.
#[derive(Default)]
pub struct Links(SyncSnapshot);

impl Links {
    pub fn new() -> Links {
        Links::default()
    }

    /// Adds `view`, linked to `tasks`.
    pub fn thread(mut self, view: ThreadView, tasks: &[i64]) -> Links {
        self.0.thread_tasks.insert(
            view.key.clone(),
            tasks.iter().copied().map(TaskId).collect(),
        );
        self.0.threads.insert(view.key.clone(), view);
        self
    }

    /// Adds `view`, linked to `tasks`.
    pub fn pr(mut self, view: PrView, tasks: &[i64]) -> Links {
        self.0.pr_tasks.insert(
            view.r#ref.clone(),
            tasks.iter().copied().map(TaskId).collect(),
        );
        self.0.prs.insert(view.r#ref.clone(), view);
        self
    }

    pub fn checked_at(mut self, at: &str) -> Links {
        self.0.checked_at = Some(at.to_string());
        self
    }

    pub fn build(self) -> SyncSnapshot {
        self.0
    }
}

pub fn inputs(core: CoreSnapshot, sync: SyncSnapshot) -> Inputs {
    Inputs {
        core: Arc::new(core),
        sync: Arc::new(sync),
    }
}

/// `compute` with the default config and nothing hidden.
pub fn run(core: CoreSnapshot, sync: SyncSnapshot, now: SystemTime) -> Computed {
    compute(&inputs(core, sync), &[], now, &AttentionConfig::default())
}

/// `compute` with these hidden rows.
pub fn run_hidden(
    core: CoreSnapshot,
    sync: SyncSnapshot,
    hidden: &[Hidden],
    now: SystemTime,
) -> Computed {
    compute(
        &inputs(core, sync),
        hidden,
        now,
        &AttentionConfig::default(),
    )
}

/// The visible keys, in order.
pub fn keys(c: &Computed) -> Vec<String> {
    c.items.iter().map(|i| i.key.0.clone()).collect()
}

/// A fresh empty directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-attention-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn db(&self) -> PathBuf {
        self.0.join("airboss.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn empty_core() -> CoreSnapshot {
    Core::new().build()
}

/// Convenience status names.
pub use Status::{Blocked, Doing, Done, Dropped, InReview, Proposed, Todo};
pub use ThreadState::{Errored, Gone, Idle, NeedsMe, Running, Settled};
