//! The view model: every row worked out from one `core` snapshot plus the services' snapshots,
//! the diff between two of them, and the plan cards.
//!
//! Nothing here reads `core`: it builds from `CoreSnapshot`, `repos()`, `sync.snapshot()` and
//! `attention.needs_me()`, so a service bump rebuilds without a `core` read.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use airboss_core::{CoreSnapshot, EventId, Repo, RepoId, Status, TaskId, TaskNode};
use airboss_services::{NeedsKind, NeedsList, SyncSnapshot, time};

use crate::dto::{
    ActorKind, ChipDto, NextUp, Patch, PlanCard, Progress, RepoDto, RowDto, StatusDto, WaveDot,
};
use crate::runmap;

/// The tree's shape, indexed once per snapshot.
#[derive(Debug, Clone, Default)]
pub struct TreeIndex {
    /// Each task's place in `CoreSnapshot.tree`.
    pub pos: HashMap<TaskId, usize>,
    /// Each parent's children (`None`: the top level), in sibling order.
    pub children: HashMap<Option<TaskId>, Vec<TaskId>>,
}

impl TreeIndex {
    /// Indexes the tree, which `core` gives in display order (a parent before its subtree).
    pub fn new(tree: &[TaskNode]) -> TreeIndex {
        let mut ix = TreeIndex::default();
        for (i, node) in tree.iter().enumerate() {
            ix.pos.insert(node.task.id, i);
            ix.children
                .entry(node.task.parent)
                .or_default()
                .push(node.task.id);
        }
        ix
    }

    /// The direct children of `parent`.
    pub fn children_of(&self, parent: TaskId) -> &[TaskId] {
        self.children
            .get(&Some(parent))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// `id` and everything under it, in tree order.
    pub fn subtree(&self, id: TaskId) -> Vec<TaskId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            out.extend_from_slice(self.children_of(out[i]));
            i += 1;
        }
        out
    }
}

/// Everything the app shows, built from one moment's snapshots.
#[derive(Debug, Clone)]
pub struct ViewModel {
    pub snapshot: Arc<CoreSnapshot>,
    pub index: TreeIndex,
    pub repos: Vec<Repo>,
    pub sync: Arc<SyncSnapshot>,
    pub needs: Arc<NeedsList>,
    pub rows: HashMap<TaskId, RowDto>,
    /// Ids in tree order.
    pub order: Vec<TaskId>,
    pub repo_dtos: Vec<RepoDto>,
}

impl ViewModel {
    /// Builds every row. `now` only turns Quiet items into whole days.
    pub fn build(
        snapshot: Arc<CoreSnapshot>,
        repos: Vec<Repo>,
        sync: Arc<SyncSnapshot>,
        needs: Arc<NeedsList>,
        now: SystemTime,
    ) -> ViewModel {
        let index = TreeIndex::new(&snapshot.tree);
        let rows = build_rows(&snapshot, &index, &repos, &sync, &needs, now);
        let order = snapshot.tree.iter().map(|n| n.task.id).collect();
        let repo_dtos = repos.iter().map(RepoDto::from).collect();
        ViewModel {
            rows: rows.into_iter().map(|r| (r.id, r)).collect(),
            snapshot,
            index,
            repos,
            sync,
            needs,
            order,
            repo_dtos,
        }
    }

    /// An empty view model, before the first snapshot.
    pub fn empty() -> ViewModel {
        ViewModel::build(
            Arc::new(CoreSnapshot {
                tree: Vec::new(),
                edges: Vec::new(),
                ready: Vec::new(),
                latest_event_id: EventId(0),
            }),
            Vec::new(),
            Arc::default(),
            Arc::default(),
            SystemTime::now(),
        )
    }

    /// The rows in tree order.
    pub fn rows_in_order(&self) -> Vec<RowDto> {
        self.order
            .iter()
            .filter_map(|id| self.rows.get(id).cloned())
            .collect()
    }

    /// One task's node in the snapshot.
    pub fn node(&self, id: TaskId) -> Option<&TaskNode> {
        self.index.pos.get(&id).map(|&i| &self.snapshot.tree[i])
    }
}

/// What changed from `old` to `new`, without a `seq` yet; `None` if nothing did.
pub fn diff(old: &ViewModel, new: &ViewModel) -> Option<Patch> {
    let upserts: Vec<RowDto> = new
        .order
        .iter()
        .filter_map(|id| new.rows.get(id))
        .filter(|row| old.rows.get(&row.id) != Some(row))
        .cloned()
        .collect();
    let mut removed: Vec<TaskId> = old
        .rows
        .keys()
        .filter(|id| !new.rows.contains_key(id))
        .copied()
        .collect();
    removed.sort_unstable();
    let created = new
        .order
        .iter()
        .filter(|id| !old.rows.contains_key(id))
        .copied()
        .collect();
    let patch = Patch {
        seq: 0,
        upserts,
        removed,
        order: (old.order != new.order).then(|| new.order.clone()),
        repos: (old.repo_dtos != new.repo_dtos).then(|| new.repo_dtos.clone()),
        created,
    };
    (!patch.is_empty()).then_some(patch)
}

/// Neither done nor dropped.
pub fn is_open(s: Status) -> bool {
    !matches!(s, Status::Done | Status::Dropped)
}

/// A leaf's own share of its ancestors' progress (DESIGN "Progress").
fn leaf_progress(s: Status) -> Progress {
    let one = |done, in_review, doing, blocked| Progress {
        done,
        total: 1,
        in_review,
        doing,
        blocked,
    };
    match s {
        Status::Dropped => Progress::default(),
        Status::Done => one(1, 0, 0, 0),
        Status::InReview => one(0, 1, 0, 0),
        Status::Doing => one(0, 0, 1, 0),
        Status::Blocked => one(0, 0, 0, 1),
        // Proposed counts as todo.
        Status::Todo | Status::Proposed => one(0, 0, 0, 0),
    }
}

fn build_rows(
    snap: &CoreSnapshot,
    index: &TreeIndex,
    repos: &[Repo],
    sync: &SyncSnapshot,
    needs: &NeedsList,
    now: SystemTime,
) -> Vec<RowDto> {
    let tree = &snap.tree;
    let n = tree.len();
    let status_of: HashMap<TaskId, Status> =
        tree.iter().map(|t| (t.task.id, t.task.status)).collect();

    // Progress and the repo union, children before parents (reverse tree order).
    let repo_rank: HashMap<RepoId, usize> =
        repos.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut progress: Vec<Progress> = vec![Progress::default(); n];
    let mut union: Vec<HashSet<RepoId>> = vec![HashSet::new(); n];
    for i in (0..n).rev() {
        let node = &tree[i];
        let id = node.task.id;
        let kids = index.children_of(id);
        let mut own: HashSet<RepoId> = node.task.repos.iter().copied().collect();
        if kids.is_empty() {
            progress[i] = leaf_progress(node.task.status);
        } else {
            let mut p = Progress::default();
            for kid in kids {
                if let Some(&k) = index.pos.get(kid) {
                    // A dropped subtree counts for nothing.
                    if tree[k].task.status != Status::Dropped {
                        p.add(progress[k]);
                    }
                    own.extend(union[k].iter().copied());
                }
            }
            progress[i] = p;
        }
        union[i] = own;
    }

    // Open blockers per waiting task.
    let mut blockers: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
    for &(waiting, waits_on) in &snap.edges {
        if status_of.get(&waits_on).is_some_and(|&s| is_open(s)) {
            blockers.entry(waiting).or_default().push(waits_on);
        }
    }

    let ready: HashMap<TaskId, &str> = snap
        .ready
        .iter()
        .map(|r| (r.task.id, r.since.as_str()))
        .collect();

    // Needs me facts per task.
    #[derive(Default)]
    struct Needs {
        any: bool,
        stale: bool,
        ready: bool,
        quiet_since: Option<SystemTime>,
    }
    let mut needs_of: HashMap<TaskId, Needs> = HashMap::new();
    for item in &needs.items {
        let e = needs_of.entry(item.task).or_default();
        e.any = true;
        e.stale |= item.stale;
        match item.kind {
            NeedsKind::Ready => e.ready = true,
            NeedsKind::Quiet => e.quiet_since = time::from_ts(&item.since),
            _ => {}
        }
    }

    // The strongest live chip per task, from one pass over the linked threads.
    let mut live: HashMap<TaskId, ChipDto> = HashMap::new();
    for (key, tasks) in &sync.thread_tasks {
        let Some(chip) = sync.threads.get(key).and_then(|t| t.state.chip()) else {
            continue;
        };
        let chip = ChipDto::from(chip);
        for task in tasks {
            let e = live.entry(*task).or_insert(chip);
            if chip.strength() > e.strength() {
                *e = chip;
            }
        }
    }

    // Branches, parents before children.
    let mut branch: Vec<Option<String>> = vec![None; n];
    for i in 0..n {
        let task = &tree[i].task;
        branch[i] = task.branch.clone().or_else(|| {
            task.parent
                .and_then(|p| index.pos.get(&p))
                .and_then(|&pi| branch[pi].clone())
        });
    }

    tree.iter()
        .enumerate()
        .map(|(i, node)| {
            let task = &node.task;
            let id = task.id;
            let has_children = !index.children_of(id).is_empty();
            let own_repos = !task.repos.is_empty();
            let mut repos_u: Vec<RepoId> = if own_repos || !has_children {
                task.repos.clone()
            } else {
                union[i].iter().copied().collect()
            };
            if !own_repos {
                repos_u.sort_by_key(|r| (repo_rank.get(r).copied().unwrap_or(usize::MAX), r.0));
            }
            let nd = needs_of.get(&id);
            let quiet_days = nd.and_then(|n| n.quiet_since).map(|since| {
                let days = now
                    .duration_since(since)
                    .unwrap_or(Duration::ZERO)
                    .as_secs()
                    / 86_400;
                u32::try_from(days).unwrap_or(u32::MAX)
            });
            RowDto {
                id,
                parent: task.parent,
                depth: node.depth,
                position: task.position,
                title: task.title.clone(),
                status: task.status.into(),
                origin: task.origin.into(),
                blocked_reason: task.blocked_reason.clone(),
                waiting: node.waiting,
                waits_on: if node.waiting {
                    blockers.get(&id).cloned().unwrap_or_default()
                } else {
                    Vec::new()
                },
                has_children,
                progress: has_children.then_some(progress[i]),
                repos_from_subtasks: !own_repos && has_children && !repos_u.is_empty(),
                repos: repos_u,
                branch: branch[i].clone(),
                ready: !has_children && ready.contains_key(&id),
                ready_since: if has_children {
                    None
                } else {
                    ready.get(&id).map(|s| s.to_string())
                },
                ready_pill: nd.is_some_and(|n| n.ready),
                quiet_days,
                needs: nd.is_some_and(|n| n.any),
                needs_stale: nd.is_some_and(|n| n.stale),
                live: if is_open(task.status) {
                    live.get(&id).copied()
                } else {
                    None
                },
                last_at: if has_children {
                    node.subtree_last_event_at.clone()
                } else {
                    node.last_event_at.clone()
                },
                last_actor: (&node.last_actor).into(),
            }
        })
        .collect()
}

/// One card per open top-level parent (Plans), in tree order.
pub fn plan_cards(vm: &ViewModel) -> Vec<PlanCard> {
    let top = vm.index.children.get(&None).cloned().unwrap_or_default();
    top.into_iter()
        .filter_map(|id| vm.rows.get(&id))
        .filter(|row| {
            row.has_children
                && !matches!(
                    row.status,
                    StatusDto::Done | StatusDto::Dropped | StatusDto::Proposed
                )
        })
        .map(|row| plan_card(vm, row))
        .collect()
}

fn plan_card(vm: &ViewModel, row: &RowDto) -> PlanCard {
    let progress = row.progress.unwrap_or_default();
    let waves = runmap::display_waves(vm, row.id);
    let open = |id: &TaskId| vm.rows.get(id).is_some_and(|r| r.status.is_open());
    let current = waves.waves.iter().position(|w| w.iter().any(open));
    let wave_dots = waves
        .waves
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if Some(i) == current {
                WaveDot::Current
            } else if w.iter().any(open) {
                WaveDot::Todo
            } else {
                WaveDot::Done
            }
        })
        .collect();
    let subtree = vm.index.subtree(row.id);
    let under = || subtree.iter().skip(1).filter_map(|id| vm.rows.get(id));
    let ready: Vec<TaskId> = under().filter(|r| r.ready).map(|r| r.id).take(2).collect();
    let next_up = if progress.total > 0 && progress.done == progress.total {
        NextUp::FinishIt
    } else if !ready.is_empty() {
        NextUp::Ready { ids: ready }
    } else {
        NextUp::Nothing
    };
    PlanCard {
        id: row.id,
        title: row.title.clone(),
        status: row.status,
        progress,
        percent: progress.percent(),
        wave_count: u32::try_from(waves.waves.len()).unwrap_or(u32::MAX),
        current_wave: current.and_then(|c| u32::try_from(c).ok()),
        wave_dots,
        next_up,
        agents_working: u32::try_from(under().filter(|r| r.live == Some(ChipDto::Working)).count())
            .unwrap_or(u32::MAX),
        blocked: progress.blocked,
        repos: row.repos.clone(),
        last_at: row.last_at.clone(),
    }
}

/// When an agent last reported anything: the newest own event by an agent on any task.
pub fn last_agent_report(vm: &ViewModel) -> Option<String> {
    vm.snapshot
        .tree
        .iter()
        .filter(|n| ActorKind::from(&n.last_actor) == ActorKind::Agent)
        .map(|n| n.last_event_at.clone())
        .max()
}
