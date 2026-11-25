//! The six MCP tools as plain functions: arguments in, a view or a [`ToolError`] out.
//!
//! They know nothing about MCP; `server.rs` wraps them. `core` decides what the agent may
//! do; these only translate names to ids, call it, and explain refusals.

use std::collections::HashSet;

use airboss_core::{Repo, RepoId, Status, Store, TaskId};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::identity::Identity;
use crate::render::{
    DependencyView, EventView, LinkView, NodeView, RepoNames, Subject, TaskDetail, TaskList,
    TaskRef, TaskView, ToolError, explain,
};

/// The most tasks `list_tasks` returns at once.
pub const LIST_LIMIT: usize = 200;

/// How many history entries `get_task` returns.
pub const HISTORY_LIMIT: usize = 20;

/// A task status, as tool arguments spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusName {
    Proposed,
    Todo,
    Doing,
    InReview,
    Blocked,
    Done,
    Dropped,
}

impl From<StatusName> for Status {
    fn from(s: StatusName) -> Status {
        match s {
            StatusName::Proposed => Status::Proposed,
            StatusName::Todo => Status::Todo,
            StatusName::Doing => Status::Doing,
            StatusName::InReview => Status::InReview,
            StatusName::Blocked => Status::Blocked,
            StatusName::Done => Status::Done,
            StatusName::Dropped => Status::Dropped,
        }
    }
}

/// Arguments for `list_tasks`. Every filter is optional, and they combine.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTasks {
    /// Keep tasks linked to any of these repos, by name.
    #[serde(default)]
    pub repos: Option<Vec<String>>,
    /// Keep tasks in these statuses. Left out: every status except done and dropped.
    #[serde(default)]
    pub status: Option<Vec<StatusName>>,
    /// Keep tasks linked to this thread.
    #[serde(default)]
    pub mine: bool,
    /// Keep tasks that can start now: todo, with nothing open to wait on.
    #[serde(default)]
    pub ready: bool,
    /// Keep tasks whose title contains this text, ignoring case.
    #[serde(default)]
    pub query: Option<String>,
}

/// Arguments for `get_task`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTask {
    /// The task's id, e.g. 12 for #12.
    pub id: i64,
}

/// Lists tasks in tree order, filtered, at most [`LIST_LIMIT`].
pub fn list_tasks(store: &Store, who: &Identity, args: ListTasks) -> Result<TaskList, ToolError> {
    let core = |e| ToolError::from_core(e);
    let repos = store.repos().map_err(core)?;
    let wanted_repos = match &args.repos {
        Some(names) => Some(repo_ids(&repos, names)?),
        None => None,
    };
    let mine = if args.mine {
        let links = who.thread_links();
        if links.is_empty() {
            return Err(ToolError::new(format!(
                "This server doesn't know which thread it serves, so `mine` can't work. \
                 Use `repos` or `query` instead. It acts as {}.",
                who.actor
            )));
        }
        let mut ids = HashSet::new();
        for (kind, r) in links {
            ids.extend(store.tasks_for_link(kind, r).map_err(core)?);
        }
        Some(ids)
    } else {
        None
    };
    let ready: Option<HashSet<TaskId>> = if args.ready {
        Some(
            store
                .ready()
                .map_err(core)?
                .into_iter()
                .map(|r| r.task.id)
                .collect(),
        )
    } else {
        None
    };
    let statuses: Option<Vec<Status>> = args
        .status
        .map(|list| list.into_iter().map(Status::from).collect());
    let query = args
        .query
        .map(|q| q.to_lowercase())
        .filter(|q| !q.is_empty());

    let names = RepoNames::new(&repos);
    let matching = store.tree().map_err(core)?.into_iter().filter(|n| {
        let t = &n.task;
        let status_ok = match &statuses {
            Some(list) => list.contains(&t.status),
            None => !matches!(t.status, Status::Done | Status::Dropped),
        };
        status_ok
            && wanted_repos
                .as_ref()
                .is_none_or(|want| t.repos.iter().any(|r| want.contains(r)))
            && mine.as_ref().is_none_or(|ids| ids.contains(&t.id))
            && ready.as_ref().is_none_or(|ids| ids.contains(&t.id))
            && query
                .as_ref()
                .is_none_or(|q| t.title.to_lowercase().contains(q))
    });
    let mut tasks = Vec::new();
    let mut total = 0;
    for n in matching {
        total += 1;
        if tasks.len() < LIST_LIMIT {
            tasks.push(NodeView {
                id: n.task.id,
                repos: names.names(&n.task.repos),
                title: n.task.title,
                status: n.task.status,
                parent: n.task.parent,
                depth: n.depth,
                waiting: n.waiting,
            });
        }
    }
    Ok(TaskList {
        truncated: total > tasks.len(),
        tasks,
        total,
    })
}

/// Everything about one task an agent needs to start or continue it.
pub fn get_task(store: &Store, args: GetTask) -> Result<TaskDetail, ToolError> {
    let id = TaskId(args.id);
    let fail = |e| explain(store, &airboss_core::Actor::Me, e, &Subject::task(id));
    let task = store.task(id).map_err(fail)?;
    let names = RepoNames::load(store)?;
    let links = store.links(id).map_err(fail)?;
    let history = store.history(id).map_err(fail)?;
    let deps = store.dependencies(id).map_err(fail)?;
    let subtasks: Vec<TaskRef> = store
        .tree()
        .map_err(fail)?
        .iter()
        .filter(|n| n.task.parent == Some(id))
        .map(|n| TaskRef::from(&n.task))
        .collect();
    let run_order = if subtasks.is_empty() {
        None
    } else {
        Some(store.run_order(id).map_err(fail)?.into())
    };
    let history_total = history.len();
    let recent = history
        .into_iter()
        .skip(history_total.saturating_sub(HISTORY_LIMIT))
        .map(EventView::from)
        .collect();
    let open = |t: &&airboss_core::Task| !matches!(t.status, Status::Done | Status::Dropped);
    Ok(TaskDetail {
        waiting: deps.waits_on.iter().any(|t| open(&t)),
        task: TaskView::new(task, &names),
        links: links.into_iter().map(LinkView::from).collect(),
        history: recent,
        history_total,
        dependencies: DependencyView {
            waits_on: deps.waits_on.iter().map(TaskRef::from).collect(),
            blocks: deps.blocks.iter().map(TaskRef::from).collect(),
        },
        subtasks,
        run_order,
    })
}

/// Maps repo names to ids; an unknown name lists the registered ones.
pub fn repo_ids(repos: &[Repo], names: &[String]) -> Result<Vec<RepoId>, ToolError> {
    names
        .iter()
        .map(|name| {
            let name = name.trim();
            repos
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.id)
                .ok_or_else(|| unknown_repo(repos, name))
        })
        .collect()
}

fn unknown_repo(repos: &[Repo], name: &str) -> ToolError {
    if repos.is_empty() {
        return ToolError::new(format!(
            "There's no repo named \"{name}\". No repos are registered yet; leave `repos` out."
        ));
    }
    let mut known: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
    known.sort_unstable();
    ToolError::new(format!(
        "There's no repo named \"{name}\". Registered repos: {}.",
        known.join(", ")
    ))
}
