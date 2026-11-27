//! The six MCP tools as plain functions: arguments in, a view or a [`ToolError`] out.
//!
//! They know nothing about MCP; `server.rs` wraps them. `core` decides what the agent may
//! do; these only translate names to ids, call it, and explain refusals.

use std::collections::HashSet;

use std::path::{Path, PathBuf};

use airboss_core::{
    Actor, DepRef, LinkKind, NewLink, NewTask, PlanTask, PrState, Priority, Repo, RepoId, Status,
    Store, TaskId, TaskNode,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::identity::Identity;
use crate::render::{
    DependencyView, EventView, LinkView, NodeView, PlanView, RepoNames, Subject, TaskDetail,
    TaskList, TaskRef, TaskView, ToolError, explain,
};

/// The most tasks `list_tasks` returns at once.
pub const LIST_LIMIT: usize = 200;

/// How many history entries `get_task` returns.
pub const HISTORY_LIMIT: usize = 20;

/// The most tasks `propose_tasks` takes at once.
pub const PROPOSE_LIMIT: usize = 20;

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

/// A priority, as tool arguments and the CLI spell it: p0 is the most urgent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum PriorityName {
    P0,
    P1,
    P2,
    P3,
}

impl From<PriorityName> for Priority {
    fn from(p: PriorityName) -> Priority {
        match p {
            PriorityName::P0 => Priority::P0,
            PriorityName::P1 => Priority::P1,
            PriorityName::P2 => Priority::P2,
            PriorityName::P3 => Priority::P3,
        }
    }
}

/// The order `list_tasks` and `airboss ls` list tasks in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum SortName {
    /// Tree order: depth-first, siblings in their chosen order.
    #[default]
    Tree,
    /// Most urgent first, P0 to P3, then tasks with no priority; ties stay in tree order.
    Priority,
}

/// Puts `nodes` most urgent first, P0 to P3 and then those with none. The sort is stable, so
/// tasks with the same priority keep the tree order they came in: siblings by position.
pub fn by_priority<N: std::borrow::Borrow<TaskNode>>(nodes: &mut [N]) {
    nodes.sort_by_key(|n| {
        let p = n.borrow().task.priority;
        (p.is_none(), p)
    });
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
    /// Keep tasks with any of these priorities. Tasks with none set are left out.
    #[serde(default)]
    pub priority: Option<Vec<PriorityName>>,
    /// `tree` (the default) or `priority`: most urgent first, ties in tree order. The 200-task
    /// cap applies after sorting.
    #[serde(default)]
    pub sort: SortName,
}

/// Arguments for `get_task`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTask {
    /// The task's id, e.g. 12 for #12.
    pub id: i64,
}

/// A status an agent may set.
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

/// Arguments for `update_task`. Give `status`, `note`, `due`, or any of them together.
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
    /// The day it's due, YYYY-MM-DD in the user's time zone, e.g. "2026-12-04" for "Friday".
    /// An empty string clears it. On that day it shows in the user's Needs me.
    #[serde(default)]
    pub due: Option<String>,
}

/// What a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum LinkKindName {
    /// A T3 Code thread. Leave `ref` out to link this thread.
    #[value(help = "A T3 Code thread, by its id")]
    T3Thread,
    /// A Claude Code session. Leave `ref` out to link this session.
    #[value(help = "A Claude Code session, by its id")]
    ClaudeSession,
    /// A terminal in Herdr, `ref` its address like t4p. Leave `ref` out to link this terminal.
    #[value(help = "A Herdr terminal, by its address, like t4p")]
    HerdrTerminal,
    /// A pull request, `ref` like owner/repo#42.
    #[value(help = "A pull request, like owner/repo#42")]
    Pr,
    /// A Slack message, `ref` its permalink.
    #[value(help = "A Slack message, by its permalink")]
    Slack,
    /// A document, `ref` its file path.
    #[value(help = "A document, by its file path")]
    Doc,
    /// A GitHub issue, `ref` like owner/repo#7.
    #[value(help = "A GitHub issue, like owner/repo#7")]
    GithubIssue,
}

impl From<LinkKindName> for LinkKind {
    fn from(k: LinkKindName) -> LinkKind {
        match k {
            LinkKindName::T3Thread => LinkKind::T3Thread,
            LinkKindName::ClaudeSession => LinkKind::ClaudeSession,
            LinkKindName::HerdrTerminal => LinkKind::HerdrTerminal,
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
    /// The thread id, terminal address, owner/repo#n, permalink or file path. Leave it out
    /// for `t3_thread`, `herdr_terminal` or `claude_session` to link this thread.
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

/// Arguments for `propose_tasks`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeTasks {
    /// The task to put them under. Left out: top level.
    #[serde(default)]
    pub parent: Option<i64>,
    /// 1 to 20 tasks, added in this order.
    pub tasks: Vec<Proposal>,
}

/// One proposed task.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    /// A short title saying what to do.
    pub title: String,
    /// Notes: why, and anything the user needs to decide.
    #[serde(default)]
    pub body: Option<String>,
    /// The repos it touches, by name. Only the user can add repos: for one that isn't
    /// registered, leave it out and start the title with its folder name ("hub-ui: …"), and
    /// Do it offers to add it.
    #[serde(default)]
    pub repos: Vec<String>,
}

/// Arguments for `import_plan`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportPlan {
    /// Path to the settled plan file, absolute or relative to this thread's directory.
    /// It must exist; it's linked to the plan's top-level tasks.
    pub plan_file: String,
    /// The task to put the plan under. Left out: top level.
    #[serde(default)]
    pub parent: Option<i64>,
    /// The plan's top-level steps, each with its own steps under it.
    pub plan: Vec<PlanStep>,
}

/// One step of a plan.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    /// The plan's own id for this step, unique in the plan. Sending the plan again matches
    /// steps by key, so keep keys stable when revising it.
    pub key: String,
    /// A short title saying what to do.
    pub title: String,
    /// Notes for whoever picks it up.
    #[serde(default)]
    pub body: Option<String>,
    /// The repos it touches, by name. Only the user can add repos: for one that isn't
    /// registered, leave it out and start the title with its folder name ("hub-ui: …"), and
    /// Do it offers to add it.
    #[serde(default)]
    pub repos: Vec<String>,
    /// What it waits on: other steps' keys, or existing task ids.
    #[serde(default)]
    pub depends_on: Vec<DependsOn>,
    /// Its own steps.
    #[serde(default)]
    pub children: Vec<PlanStep>,
}

/// Another step's key, or an existing task's id.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DependsOn {
    Key(String),
    Task(i64),
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
    let priorities: Option<Vec<Priority>> = args
        .priority
        .map(|list| list.into_iter().map(Priority::from).collect());

    let names = RepoNames::new(&repos);
    let mut matching: Vec<TaskNode> = store
        .tree()
        .map_err(core)?
        .into_iter()
        .filter(|n| {
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
                && priorities
                    .as_ref()
                    .is_none_or(|want| t.priority.is_some_and(|p| want.contains(&p)))
        })
        .collect();
    if args.sort == SortName::Priority {
        by_priority(&mut matching);
    }
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
                priority: n.task.priority,
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
    let fail = |e| explain(store, &Actor::Me, e, &Subject::task(id));
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
    Ok(TaskDetail {
        waiting: deps
            .waits_on
            .iter()
            .any(|t| !matches!(t.status, Status::Done | Status::Dropped)),
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
    // The date first: a bad one is the likeliest mistake, and then nothing else is written.
    let dated = match &args.due {
        Some(due) => Some(store.set_due(id, Some(due), actor).map_err(|e| match e {
            airboss_core::Error::InvalidTarget => ToolError::bad_due(),
            e => explain(store, actor, e, &Subject::task(id)),
        })?),
        None => None,
    };
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
        (None, None) => match dated {
            Some(task) => task,
            None => return Err(ToolError::new("Give `status`, `note`, `due`, or several.")),
        },
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
        .map_err(|e| match e {
            airboss_core::Error::InvalidTarget if kind == LinkKind::Pr => ToolError::bad_pr_ref(),
            e => explain(store, &who.actor, e, &Subject::task(id)),
        })
}

fn missing_ref(kind: LinkKind) -> ToolError {
    ToolError::new(match kind {
        LinkKind::T3Thread => "This server doesn't know its T3 thread id, so give `ref`.",
        LinkKind::ClaudeSession => {
            "This server doesn't know its Claude Code session id, so give `ref`."
        }
        LinkKind::HerdrTerminal => {
            "This server doesn't know its Herdr terminal, so give `ref`: its address, like t4p."
        }
        LinkKind::Pr => "Give `ref` for a pr link, e.g. owner/repo#42.",
        LinkKind::GithubIssue => "Give `ref` for a github_issue link, e.g. owner/repo#7.",
        LinkKind::Slack => "Give `ref` for a slack link: the message's permalink.",
        LinkKind::Doc => "Give `ref` for a doc link: the file's path.",
    })
}

/// Proposes new work. Every task arrives `proposed` and waits for the user to accept it.
/// Everything is checked before the first one is added.
pub fn propose_tasks(
    store: &mut Store,
    who: &Identity,
    args: ProposeTasks,
) -> Result<Vec<TaskView>, ToolError> {
    if args.tasks.is_empty() || args.tasks.len() > PROPOSE_LIMIT {
        return Err(ToolError::new(format!(
            "Propose between 1 and {PROPOSE_LIMIT} tasks at a time."
        )));
    }
    let actor = &who.actor;
    let parent = args.parent.map(TaskId);
    let under = Subject {
        parent,
        ..Subject::default()
    };
    if let Some(p) = parent {
        let status = store
            .task(p)
            .map_err(|e| explain(store, actor, e, &under))?
            .status;
        if matches!(status, Status::Proposed | Status::Done | Status::Dropped) {
            return Err(explain(
                store,
                actor,
                airboss_core::Error::InvalidTarget,
                &under,
            ));
        }
    }
    let repos = store.repos().map_err(ToolError::from_core)?;
    let mut new = Vec::with_capacity(args.tasks.len());
    for p in args.tasks {
        if p.title.trim().is_empty() {
            return Err(ToolError::from_core(airboss_core::Error::EmptyTitle));
        }
        let mut task = NewTask::new(p.title);
        task.parent = parent;
        task.body = p.body.filter(|b| !b.trim().is_empty());
        for id in repo_ids(&repos, &p.repos)? {
            task = task.in_repo(id);
        }
        new.push(task);
    }
    let names = RepoNames::new(&repos);
    let mut added: Vec<TaskView> = Vec::with_capacity(new.len());
    for task in new {
        match store.add_task(task, actor) {
            Ok(t) => added.push(TaskView::new(t, &names)),
            Err(e) => {
                let ToolError(why) = explain(store, actor, e, &under);
                let done: Vec<String> = added.iter().map(|t| format!("#{}", t.id.0)).collect();
                return Err(ToolError::new(if done.is_empty() {
                    why
                } else {
                    format!("{why} (Already proposed: {}.)", done.join(", "))
                }));
            }
        }
    }
    Ok(added)
}

/// Imports a plan the user settled in plan-review, as `plan`, so its tasks start `todo`.
/// The plan file is linked to the top-level tasks, and each new task gets a note naming
/// the thread that sent it.
pub fn import_plan(
    store: &mut Store,
    who: &Identity,
    args: ImportPlan,
) -> Result<PlanView, ToolError> {
    if args.plan.is_empty() {
        return Err(ToolError::new("Give `plan` at least one step."));
    }
    let file = plan_file(&args.plan_file)?;
    let repos = store.repos().map_err(ToolError::from_core)?;
    let mut others = Vec::new();
    let plan = args
        .plan
        .into_iter()
        .map(|step| plan_task(step, &repos, &mut others))
        .collect::<Result<Vec<_>, _>>()?;
    let parent = args.parent.map(TaskId);
    let about = Subject {
        parent,
        others,
        ..Subject::default()
    };
    let result = store
        .add_tasks(parent, &plan, &Actor::Plan)
        .map_err(|e| explain(store, &who.actor, e, &about))?;

    let path = file.to_string_lossy().into_owned();
    let title = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let after = |store: &Store, e| {
        let ToolError(why) = explain(store, &who.actor, e, &Subject::default());
        ToolError::new(format!(
            "The plan was imported, but finishing it failed: {why}"
        ))
    };
    for step in &plan {
        if let Some(id) = result.id(&step.key) {
            let doc = NewLink::new(LinkKind::Doc, path.clone()).with_title(title.clone());
            store
                .link(id, doc, &Actor::Plan)
                .map_err(|e| after(store, e))?;
        }
    }
    // The note's actor is the sending thread, so the body needn't repeat it; the file name is
    // enough to read, and the top-level tasks link the full path.
    let note = format!("Imported from {title}");
    for &id in &result.created {
        store
            .add_note(id, &note, &who.actor)
            .map_err(|e| after(store, e))?;
    }
    Ok(PlanView {
        keys: result.keys.iter().cloned().collect(),
        created: result.created,
        existing: result.existing,
    })
}

/// The plan file as an absolute path, if it exists.
fn plan_file(given: &str) -> Result<PathBuf, ToolError> {
    let missing = || {
        ToolError::new(format!(
            "There's no file at \"{given}\". Give the path of the plan file the user \
             settled in plan-review."
        ))
    };
    let path = Path::new(given.trim());
    if given.trim().is_empty() || !path.is_file() {
        return Err(missing());
    }
    path.canonicalize().map_err(|_| missing())
}

/// One plan step in `core`'s terms, with repo names resolved. Collects the existing
/// task ids it waits on, so a missing one can be named.
fn plan_task(
    step: PlanStep,
    repos: &[Repo],
    others: &mut Vec<TaskId>,
) -> Result<PlanTask, ToolError> {
    let mut task = PlanTask::new(step.key, step.title);
    task.body = step.body.filter(|b| !b.trim().is_empty());
    for id in repo_ids(repos, &step.repos)? {
        task = task.in_repo(id);
    }
    for dep in step.depends_on {
        task.depends_on.push(match dep {
            DependsOn::Key(key) => DepRef::Key(key),
            DependsOn::Task(id) => {
                others.push(TaskId(id));
                DepRef::Task(TaskId(id))
            }
        });
    }
    for child in step.children {
        task = task.child(plan_task(child, repos, others)?);
    }
    Ok(task)
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
