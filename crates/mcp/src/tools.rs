//! The six MCP tools as plain functions: arguments in, a view or a [`ToolError`] out.
//!
//! They know nothing about MCP; `server.rs` wraps them. `core` decides what the agent may
//! do; these only translate names to ids, call it, and explain refusals.

use std::collections::HashSet;

use airboss_core::{LinkKind, NewLink, PrState, Repo, RepoId, Status, Store, TaskId};
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

/// A status an agent may set. `core` decides whether it may set it on this task now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Working on it.
    Doing,
    /// Waiting for review, e.g. its PR is open.
    InReview,
    /// Stuck; `note` says why.
    Blocked,
    /// Finished. Only for a task with no PR, and `note` says what was done.
    Done,
}

impl From<AgentStatus> for Status {
    fn from(s: AgentStatus) -> Status {
        match s {
            AgentStatus::Doing => Status::Doing,
            AgentStatus::InReview => Status::InReview,
            AgentStatus::Blocked => Status::Blocked,
            AgentStatus::Done => Status::Done,
        }
    }
}

/// Arguments for `update_task`. Give `status`, `note`, or both.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTask {
    /// The task's id.
    pub id: i64,
    /// The new status.
    #[serde(default)]
    pub status: Option<AgentStatus>,
    /// With `status`: why it's blocked, or what was done. Alone: a progress note.
    #[serde(default)]
    pub note: Option<String>,
}

/// What a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkKindName {
    /// A T3 Code thread. Leave `ref` out to link this thread.
    T3Thread,
    /// A Claude Code session. Leave `ref` out to link this session.
    ClaudeSession,
    /// A pull request, `ref` like owner/repo#42.
    Pr,
    /// A Slack message, `ref` its permalink.
    Slack,
    /// A document, `ref` its file path.
    Doc,
    /// A GitHub issue, `ref` like owner/repo#7.
    GithubIssue,
}

impl From<LinkKindName> for LinkKind {
    fn from(k: LinkKindName) -> LinkKind {
        match k {
            LinkKindName::T3Thread => LinkKind::T3Thread,
            LinkKindName::ClaudeSession => LinkKind::ClaudeSession,
            LinkKindName::Pr => LinkKind::Pr,
            LinkKindName::Slack => LinkKind::Slack,
            LinkKindName::Doc => LinkKind::Doc,
            LinkKindName::GithubIssue => LinkKind::GithubIssue,
        }
    }
}

/// A PR's state, for a PR link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrStateName {
    Open,
    Merged,
    Closed,
}

impl From<PrStateName> for PrState {
    fn from(s: PrStateName) -> PrState {
        match s {
            PrStateName::Open => PrState::Open,
            PrStateName::Merged => PrState::Merged,
            PrStateName::Closed => PrState::Closed,
        }
    }
}

/// Arguments for `link`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// The task's id.
    pub id: i64,
    /// What the link points at.
    pub kind: LinkKindName,
    /// The thread id, owner/repo#n, permalink or file path. Leave it out for
    /// `t3_thread` or `claude_session` to link this thread.
    #[serde(default, rename = "ref")]
    pub r#ref: Option<String>,
    /// The web page it lives at, if any.
    #[serde(default)]
    pub url: Option<String>,
    /// For a `pr` link only: its state if already known. Left out, it starts open.
    #[serde(default)]
    pub pr_state: Option<PrStateName>,
    /// A short name to show instead of the ref, e.g. "booking-flow.md".
    #[serde(default)]
    pub title: Option<String>,
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

/// Reports progress: a status change, a note, or both. `core` decides what's allowed.
pub fn update_task(
    store: &mut Store,
    who: &Identity,
    args: UpdateTask,
) -> Result<TaskView, ToolError> {
    let id = TaskId(args.id);
    let note = args.note.as_deref().filter(|n| !n.trim().is_empty());
    let actor = &who.actor;
    let task = match (args.status, note) {
        (Some(to), note) => {
            let to = Status::from(to);
            store.set_status(id, to, note, actor).map_err(|e| {
                let about = Subject {
                    to: Some(to),
                    ..Subject::task(id)
                };
                explain(store, actor, e, &about)
            })?
        }
        (None, Some(note)) => {
            let fail = |store: &Store, e| explain(store, actor, e, &Subject::task(id));
            store
                .add_note(id, note, actor)
                .map_err(|e| fail(store, e))?;
            store.task(id).map_err(|e| fail(store, e))?
        }
        (None, None) => return Err(ToolError::new("Give `status`, `note`, or both.")),
    };
    Ok(TaskView::new(task, &RepoNames::load(store)?))
}

/// Links something to a task; a thread link with no `ref` links this thread.
pub fn link(store: &mut Store, who: &Identity, args: Link) -> Result<LinkView, ToolError> {
    let id = TaskId(args.id);
    let kind = LinkKind::from(args.kind);
    let given = args
        .r#ref
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    let r#ref = match (given, who.thread_ref(kind)) {
        (Some(r), _) => r,
        (None, Some(own)) => own.to_string(),
        (None, None) => return Err(missing_ref(kind)),
    };
    if args.pr_state.is_some() && kind != LinkKind::Pr {
        return Err(ToolError::new(
            "`pr_state` only goes with a `pr` link; leave it out.",
        ));
    }
    let mut new = NewLink::new(kind, r#ref);
    new.url = args.url.filter(|u| !u.trim().is_empty());
    new.pr_state = args.pr_state.map(PrState::from);
    new.title = args.title;
    store
        .link(id, new, &who.actor)
        .map(LinkView::from)
        .map_err(|e| explain(store, &who.actor, e, &Subject::task(id)))
}

fn missing_ref(kind: LinkKind) -> ToolError {
    ToolError::new(match kind {
        LinkKind::T3Thread => "This server doesn't know its T3 thread id, so give `ref`.",
        LinkKind::ClaudeSession => {
            "This server doesn't know its Claude Code session id, so give `ref`."
        }
        LinkKind::Pr => "Give `ref` for a pr link, e.g. owner/repo#42.",
        LinkKind::GithubIssue => "Give `ref` for a github_issue link, e.g. owner/repo#7.",
        LinkKind::Slack => "Give `ref` for a slack link: the message's permalink.",
        LinkKind::Doc => "Give `ref` for a doc link: the file's path.",
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
