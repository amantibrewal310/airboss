//! `prepare`, and the task facts `start` and the briefs read from `core`.

use std::collections::{HashMap, HashSet};

use airboss_core::{Dependencies, Link, Repo, RunOrder, Status, Task, TaskId, TaskNode};
use airboss_services::{Mode, Prepared, ServiceError, Source, StartedThread, ThreadKey};

use crate::brief::{Reference, SubtaskLine, TaskLine};
use crate::db::{Row, State};
use crate::{Inner, branch, errors, lock};

/// Everything about one task launch needs, read under one store lock.
pub(crate) struct Facts {
    pub task: Task,
    pub tree: Vec<TaskNode>,
    pub waits_on: Vec<Task>,
    pub deps: Dependencies,
    pub links: Vec<Link>,
    /// Every registered repo, by name.
    pub repos: Vec<Repo>,
    pub run_order: Option<RunOrder>,
}

/// A `core` error about one task, for the user.
pub(crate) fn core_error(task: TaskId, e: &airboss_core::Error) -> ServiceError {
    match e {
        airboss_core::Error::NotFound => errors::not_found(task),
        other => errors::core(other),
    }
}

impl Facts {
    pub fn load(inner: &Inner, id: TaskId) -> Result<Facts, ServiceError> {
        inner
            .core
            .read(|s| {
                let task = s.task(id)?;
                let tree = s.tree()?;
                let has_children = tree.iter().any(|n| n.task.parent == Some(id));
                Ok(Facts {
                    waits_on: s.waits_on(id)?,
                    deps: s.dependencies(id)?,
                    links: s.links(id)?,
                    repos: s.repos()?,
                    run_order: if has_children {
                        Some(s.run_order(id)?)
                    } else {
                        None
                    },
                    task,
                    tree,
                })
            })
            .map_err(|e| core_error(id, &e))
    }

    /// `TaskProposed` or `TaskClosed`, for a task neither button starts.
    pub fn check_open(&self) -> Result<(), ServiceError> {
        match self.task.status {
            Status::Proposed => Err(errors::task_proposed(self.task.id)),
            s @ (Status::Done | Status::Dropped) => Err(errors::task_closed(self.task.id, s)),
            _ => Ok(()),
        }
    }

    /// The task's repos, or the union of its open subtasks' (at any depth) when it has
    /// none, by name; and whether they came from the subtasks.
    pub fn task_repos(&self) -> (Vec<Repo>, bool) {
        let pick = |ids: &HashSet<_>| -> Vec<Repo> {
            self.repos
                .iter()
                .filter(|r| ids.contains(&r.id))
                .cloned()
                .collect()
        };
        if !self.task.repos.is_empty() {
            return (pick(&self.task.repos.iter().copied().collect()), false);
        }
        let ids: HashSet<_> = self
            .descendants()
            .into_iter()
            .filter(|t| !matches!(t.status, Status::Done | Status::Dropped | Status::Proposed))
            .flat_map(|t| t.repos.iter().copied())
            .collect();
        let repos = pick(&ids);
        let from_subtasks = !repos.is_empty();
        (repos, from_subtasks)
    }

    /// Every task under this one, in tree order.
    fn descendants(&self) -> Vec<&Task> {
        let mut inside: HashSet<TaskId> = HashSet::from([self.task.id]);
        let mut out = Vec::new();
        // The tree is depth-first, so a parent always comes before its subtasks.
        for node in &self.tree {
            if let Some(parent) = node.task.parent
                && inside.contains(&parent)
            {
                inside.insert(node.task.id);
                out.push(&node.task);
            }
        }
        out
    }

    /// The branch Do it uses: the task's own, or `task-<id>-<slug>`.
    pub fn branch(&self) -> String {
        self.task
            .branch
            .clone()
            .unwrap_or_else(|| branch::task_branch(self.task.id, &self.task.title))
    }

    pub fn repo_names(&self) -> HashMap<airboss_core::RepoId, &str> {
        self.repos.iter().map(|r| (r.id, r.name.as_str())).collect()
    }

    /// What the reference block shows.
    pub fn reference(&self) -> Reference {
        let by_id: HashMap<TaskId, &Task> =
            self.tree.iter().map(|n| (n.task.id, &n.task)).collect();
        let line = |t: &Task| TaskLine {
            id: t.id,
            title: t.title.clone(),
            status: t.status,
        };
        let mut under = Vec::new();
        let mut at = self.task.parent;
        while let Some(id) = at {
            match by_id.get(&id) {
                Some(t) => {
                    under.push(line(t));
                    at = t.parent;
                }
                None => break,
            }
            if under.len() > 100 {
                break;
            }
        }
        under.reverse();
        let parent_notes = self.task.parent.and_then(|p| by_id.get(&p)).and_then(|p| {
            let notes = p.body.as_deref()?.trim();
            (!notes.is_empty()).then(|| (line(p), notes.to_string()))
        });
        let names = self.repo_names();
        let sub = |t: &Task| SubtaskLine {
            id: t.id,
            title: t.title.clone(),
            status: t.status,
            repos: t
                .repos
                .iter()
                .filter_map(|r| names.get(r).map(|n| n.to_string()))
                .collect(),
        };
        // Open subtasks in run order, then the rest in tree order.
        let mut subtasks: Vec<SubtaskLine> = Vec::new();
        let mut seen = HashSet::new();
        if let Some(order) = &self.run_order {
            let ordered = order
                .waves
                .iter()
                .flatten()
                .chain(order.waiting_outside.iter().map(|w| &w.task))
                .chain(order.tangled.iter());
            for t in ordered {
                if seen.insert(t.id) {
                    subtasks.push(sub(t));
                }
            }
        }
        for node in &self.tree {
            let t = &node.task;
            if t.parent == Some(self.task.id) && t.status != Status::Proposed && seen.insert(t.id) {
                subtasks.push(sub(t));
            }
        }
        Reference {
            task: self.task.id,
            title: self.task.title.clone(),
            notes: self.task.body.clone(),
            under,
            parent_notes,
            subtasks,
            waits_on: self.deps.waits_on.iter().map(line).collect(),
            blocks: self.deps.blocks.iter().map(line).collect(),
            links: self.links.clone(),
        }
    }
}

/// A started row as the app shows it.
pub(crate) fn started_thread(row: &Row) -> Option<StartedThread> {
    let thread = row.thread_id.as_ref()?;
    (row.state == State::Started).then(|| StartedThread {
        key: ThreadKey {
            source: Source::T3,
            r#ref: thread.clone(),
        },
        mode: row.mode,
        repo: row.repo,
        title: row.title.clone(),
        linked: row.linked,
    })
}

/// `LaunchService::prepare`: reads only, runs no process.
pub(crate) fn prepare(inner: &Inner, task: TaskId, mode: Mode) -> Result<Prepared, ServiceError> {
    let facts = Facts::load(inner, task)?;
    facts.check_open()?;
    let (repos, from_subtasks) = facts.task_repos();
    let rows = lock(&inner.db).rows_for_task(task)?;
    let branch = (mode == Mode::DoIt && !repos.is_empty()).then(|| facts.branch());
    Ok(Prepared {
        needs_choice: mode == Mode::DoIt && repos.is_empty(),
        repos,
        from_subtasks,
        choices: facts.repos.clone(),
        branch,
        research_dir: inner.cfg.research_dir.clone(),
        waiting_on: facts.waits_on.iter().map(|t| t.id).collect(),
        started: rows.iter().filter_map(started_thread).collect(),
    })
}
