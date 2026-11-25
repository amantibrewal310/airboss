//! Shapes shared by the tools and the CLI: the JSON views of tasks, links and history, and
//! the one message each `core` error turns into.
//!
//! An agent and I see the same shapes: tools return them as JSON text, and the CLI prints
//! them with `--json`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use airboss_core::{
    Actor, Error, Event, EventId, EventKind, Link, LinkId, LinkKind, Origin, PrState, Repo, RepoId,
    Status, Store, Task, TaskId,
};
use serde::Serialize;

/// Repo names by id, read once per call.
pub struct RepoNames(HashMap<RepoId, String>);

impl RepoNames {
    pub fn new(repos: &[Repo]) -> RepoNames {
        RepoNames(repos.iter().map(|r| (r.id, r.name.clone())).collect())
    }

    pub fn load(store: &Store) -> Result<RepoNames, ToolError> {
        Ok(RepoNames::new(
            &store.repos().map_err(ToolError::from_core)?,
        ))
    }

    /// The names of `ids`, keeping their order (core sorts a task's repos by name).
    pub fn names(&self, ids: &[RepoId]) -> Vec<String> {
        ids.iter()
            .map(|id| {
                self.0
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("repo {}", id.0))
            })
            .collect()
    }
}

/// One task, as `update_task`, `propose_tasks` and `get_task` show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskView {
    pub id: TaskId,
    pub title: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<TaskId>,
    pub repos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub origin: Origin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_key: Option<String>,
    pub created_at: String,
}

impl TaskView {
    pub fn new(task: Task, repos: &RepoNames) -> TaskView {
        TaskView {
            id: task.id,
            repos: repos.names(&task.repos),
            title: task.title,
            status: task.status,
            blocked_reason: task.blocked_reason,
            parent: task.parent,
            branch: task.branch,
            body: task.body,
            origin: task.origin,
            plan_key: task.plan_key,
            created_at: task.created_at,
        }
    }
}

/// One row of `list_tasks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeView {
    pub id: TaskId,
    pub title: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<TaskId>,
    pub depth: u32,
    pub waiting: bool,
    pub repos: Vec<String>,
}

/// What `list_tasks` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskList {
    pub tasks: Vec<NodeView>,
    /// How many tasks matched, before the cap.
    pub total: usize,
    /// True when more matched than are listed.
    pub truncated: bool,
}

/// Another task, named in passing: a subtask, a dependency, a run-order entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskRef {
    pub id: TaskId,
    pub title: String,
    pub status: Status,
}

impl From<&Task> for TaskRef {
    fn from(task: &Task) -> TaskRef {
        TaskRef {
            id: task.id,
            title: task.title.clone(),
            status: task.status,
        }
    }
}

/// One link on a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkView {
    pub id: LinkId,
    pub task: TaskId,
    pub kind: LinkKind,
    pub r#ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_state: Option<PrState>,
    pub linked_by: Actor,
    pub linked_at: String,
}

impl From<Link> for LinkView {
    fn from(link: Link) -> LinkView {
        LinkView {
            id: link.id,
            task: link.task,
            kind: link.kind,
            r#ref: link.r#ref,
            title: link.title,
            url: link.url,
            pr_state: link.pr_state,
            linked_by: link.linked_by,
            linked_at: link.linked_at,
        }
    }
}

/// One history entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventView {
    pub id: EventId,
    pub task: TaskId,
    pub at: String,
    pub actor: Actor,
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl From<Event> for EventView {
    fn from(e: Event) -> EventView {
        EventView {
            id: e.id,
            task: e.task,
            at: e.at,
            actor: e.actor,
            kind: e.kind,
            to_status: e.to_status,
            body: e.body,
        }
    }
}

/// Both directions of a task's dependencies, finished ones included.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DependencyView {
    pub waits_on: Vec<TaskRef>,
    pub blocks: Vec<TaskRef>,
}

/// A subtask held back by open tasks outside its parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutsideView {
    pub task: TaskRef,
    pub blockers: Vec<TaskId>,
}

/// The order a parent's open subtasks can run in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RunOrderView {
    pub waves: Vec<Vec<TaskRef>>,
    pub waiting_outside: Vec<OutsideView>,
    pub tangled: Vec<TaskRef>,
}

impl From<airboss_core::RunOrder> for RunOrderView {
    fn from(order: airboss_core::RunOrder) -> RunOrderView {
        RunOrderView {
            waves: order
                .waves
                .iter()
                .map(|w| w.iter().map(TaskRef::from).collect())
                .collect(),
            waiting_outside: order
                .waiting_outside
                .iter()
                .map(|o| OutsideView {
                    task: TaskRef::from(&o.task),
                    blockers: o.blockers.clone(),
                })
                .collect(),
            tangled: order.tangled.iter().map(TaskRef::from).collect(),
        }
    }
}

/// Everything `get_task` and `airboss show` know about one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskDetail {
    pub task: TaskView,
    /// True while any task it waits on is neither done nor dropped.
    pub waiting: bool,
    pub links: Vec<LinkView>,
    /// The newest entries, oldest first.
    pub history: Vec<EventView>,
    /// How many entries its whole history has.
    pub history_total: usize,
    pub dependencies: DependencyView,
    /// Its direct subtasks, in order.
    pub subtasks: Vec<TaskRef>,
    /// Only when it has subtasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_order: Option<RunOrderView>,
}

/// What `import_plan` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanView {
    pub created: Vec<TaskId>,
    pub existing: Vec<TaskId>,
    /// Every step's key and the task it now is.
    pub keys: BTreeMap<String, TaskId>,
}

/// A call refused or failed, with one sentence that says what to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

impl ToolError {
    pub fn new(message: impl Into<String>) -> ToolError {
        ToolError(message.into())
    }

    /// A `core` error with nothing more known about the call; prefer [`explain`].
    pub fn from_core(err: Error) -> ToolError {
        ToolError(message(&err, &Facts::default()))
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

/// What a failed call was about, so its message can name things.
#[derive(Debug, Clone, Default)]
pub struct Subject {
    /// The task the call changed or read.
    pub task: Option<TaskId>,
    /// The status it tried to set.
    pub to: Option<Status>,
    /// The parent it tried to put something under.
    pub parent: Option<TaskId>,
    /// Other tasks it named, e.g. what to wait on.
    pub others: Vec<TaskId>,
    /// The repo it named.
    pub repo: Option<String>,
}

impl Subject {
    pub fn task(id: TaskId) -> Subject {
        Subject {
            task: Some(id),
            ..Subject::default()
        }
    }
}

/// The facts a message needs, read from the store only after a call has failed.
#[derive(Debug, Clone, Default)]
struct Facts {
    actor_is_me: bool,
    task: Option<(TaskId, Option<Status>, bool)>,
    to: Option<Status>,
    parent: Option<(TaskId, Option<Status>)>,
    missing: Option<TaskId>,
    repo: Option<String>,
}

/// Turns a `core` error into its message, reading whatever the message needs to name.
pub fn explain(store: &Store, actor: &Actor, err: Error, about: &Subject) -> ToolError {
    let status = |id: TaskId| store.task(id).ok().map(|t| t.status);
    let has_pr = |id: TaskId| {
        store
            .links(id)
            .map(|ls| ls.iter().any(|l| l.kind == LinkKind::Pr))
            .unwrap_or(false)
    };
    let mut named = about
        .task
        .into_iter()
        .chain(about.parent)
        .chain(about.others.iter().copied());
    let facts = Facts {
        actor_is_me: *actor == Actor::Me,
        task: about.task.map(|id| (id, status(id), has_pr(id))),
        to: about.to,
        parent: about.parent.map(|id| (id, status(id))),
        missing: match err {
            Error::NotFound => named.find(|&id| status(id).is_none()),
            _ => None,
        },
        repo: about.repo.clone(),
    };
    ToolError(message(&err, &facts))
}

/// One fixed message per `core` error variant, filled in with what the call was about.
fn message(err: &Error, facts: &Facts) -> String {
    let task = |f: &dyn Fn(TaskId, Status) -> String| match facts.task {
        Some((id, Some(status), _)) => Some(f(id, status)),
        _ => None,
    };
    match err {
        Error::EmptyTitle => "Give every task a title that isn't blank.".into(),
        Error::AlreadyExists => {
            "That name or path is already registered. List the repos to see it.".into()
        }
        Error::NotFound => match (facts.missing, &facts.repo, facts.task) {
            (Some(id), _, _) => {
                format!(
                    "There's no task #{}. List the tasks to find the right id.",
                    id.0
                )
            }
            (None, Some(repo), Some((id, ..))) => {
                format!("Task #{} isn't linked to the repo \"{repo}\".", id.0)
            }
            _ => "That doesn't exist. Check the id and try again.".into(),
        },
        Error::NotAllowed => match (facts.to, facts.task) {
            (Some(Status::Done), Some((_, _, true))) if !facts.actor_is_me => {
                "Agents can't finish a task that has a PR; it finishes when the PR merges. \
                 Set it to in_review instead."
                    .into()
            }
            (Some(Status::Proposed), _) if facts.actor_is_me => {
                "Only agents propose tasks. Set it to todo instead.".into()
            }
            (Some(to), _) => format!(
                "Agents can set doing, in_review, blocked, or done (on a task with no PR); \
                 only the user can set {to}."
            ),
            (None, _) => "Only the user can make this change, from the Airboss CLI or app.".into(),
        },
        Error::InvalidTransition => task(&|id, status| {
            let next = match status {
                Status::Proposed => "Wait for the user to accept it.",
                Status::Done | Status::Dropped => {
                    "Ask the user to reopen it if there's more to do."
                }
                _ => "Pick a status it can move to from there.",
            };
            format!(
                "Task #{} is {status}, so its status can't change from here. {next}",
                id.0
            )
        })
        .unwrap_or_else(|| "The task's status can't change from where it is now.".into()),
        Error::NoteRequired => match facts.to {
            Some(Status::Blocked) => "Say why it's blocked in `note`.".into(),
            _ => "Say what you did in `note` when finishing a task.".into(),
        },
        Error::InvalidTarget => match facts.parent {
            Some((id, Some(status @ (Status::Proposed | Status::Done | Status::Dropped)))) => {
                format!(
                    "Task #{} is {status}, so nothing can go under it. Pick an open parent \
                     or none.",
                    id.0
                )
            }
            _ => "A task can't wait on its own parent or subtask. Remove that dependency.".into(),
        },
        Error::Loop => match (facts.parent, facts.task) {
            (Some((parent, _)), Some((id, ..))) => format!(
                "Task #{} is inside #{}, so #{} can't move under it.",
                parent.0, id.0, id.0
            ),
            _ => "That would make a loop of tasks waiting on each other. Remove one of the \
                  dependencies."
                .into(),
        },
        Error::DuplicateKey(key) => {
            format!("The plan uses the key \"{key}\" twice. Give every step its own key.")
        }
        // `mcp` doesn't depend on rusqlite, so busy is recognised by SQLite's own wording.
        Error::Db(e) if is_busy(&e.to_string()) => {
            "The Airboss database is busy. Try again in a moment.".into()
        }
        Error::Db(e) => format!(
            "The Airboss database failed ({e}). Try again; if it keeps failing, tell the user."
        ),
    }
}

fn is_busy(text: &str) -> bool {
    text.contains("database is locked") || text.contains("database is busy")
}
