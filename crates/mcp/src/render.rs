//! Shapes shared by the tools and the CLI: the JSON views of tasks, links and history, and
//! the one message each `core` error turns into.
//!
//! An agent and I see the same shapes: tools return them as JSON text, and the CLI prints
//! them with `--json`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::IsTerminal;

use airboss_core::{
    Actor, Error, Event, EventId, EventKind, Link, LinkId, LinkKind, Origin, PrState, Priority,
    Repo, RepoId, Status, Store, Task, TaskId,
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
    pub priority: Option<Priority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<TaskId>,
    pub repos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The date it's due, `YYYY-MM-DD` in the user's time zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at: Option<String>,
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
            priority: task.priority,
            blocked_reason: task.blocked_reason,
            parent: task.parent,
            branch: task.branch,
            due_at: task.due_at,
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
    pub priority: Option<Priority>,
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

    /// `core` refused a branch name (core rule 25): say what one may be.
    pub fn bad_branch() -> ToolError {
        ToolError::new(
            "A branch name uses only letters, digits, and . _ / -, is at most 200 \
             characters, and can't contain .., start with - or /, or end with / or .lock.",
        )
    }

    /// `core` refused a due date (core rule 36): say what one looks like.
    pub fn bad_due() -> ToolError {
        ToolError::new(
            "A due date is a real day written YYYY-MM-DD, e.g. 2026-12-04. Work out \"Friday\" \
             or \"in 2 days\" from today's date in the user's time zone first.",
        )
    }

    /// `core` refused a PR ref (core rule 29): say what one looks like.
    pub fn bad_pr_ref() -> ToolError {
        ToolError::new(
            "A PR is owner/repo#42 or its GitHub URL, like \
             https://github.com/owner/repo/pull/42.",
        )
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
        Error::HasSteps => match facts.task {
            Some((id, ..)) => format!(
                "Task #{} has subtasks, so its status follows them, and PRs and branches go on \
                 the subtasks. Report on the subtask you're working on.",
                id.0
            ),
            None => "That task has subtasks, so its status follows them, and PRs and branches \
                     go on the subtasks. Report on the subtask you're working on."
                .into(),
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
        Error::Db(_) if err.is_newer_database() => airboss_core::NEWER_DATABASE.into(),
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

// ---- text, for the CLI ----

/// Whether to colour output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub color: bool,
}

impl Style {
    /// Colour only when stdout is a terminal and `NO_COLOR` is unset or empty.
    pub fn decide(stdout_is_tty: bool, no_color: Option<&str>) -> Style {
        Style {
            color: stdout_is_tty && no_color.is_none_or(str::is_empty),
        }
    }

    pub fn detect() -> Style {
        let no_color = std::env::var("NO_COLOR").ok();
        Style::decide(std::io::stdout().is_terminal(), no_color.as_deref())
    }

    pub fn paint(self, code: &str, text: &str) -> String {
        if self.color && !text.is_empty() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn dim(self, text: &str) -> String {
        self.paint("2", text)
    }

    pub fn bold(self, text: &str) -> String {
        self.paint("1", text)
    }
}

/// How a status looks: DESIGN.md's shape, its colour, and its name. `waiting` is a todo
/// task with an open blocker, which has its own look though it's never set.
pub fn mark(status: Status, waiting: bool) -> (&'static str, &'static str, &'static str) {
    match status {
        Status::Todo if waiting => ("○", "38;5;245", "Waiting"),
        Status::Todo => ("○", "38;5;245", "Todo"),
        Status::Proposed => ("◌", "38;5;245", "Proposed"),
        Status::Doing => ("◑", "38;5;214", "In progress"),
        Status::InReview => ("◕", "38;5;35", "In review"),
        Status::Blocked => ("⊖", "38;5;208", "Blocked"),
        Status::Done => ("●", "38;5;105", "Done"),
        Status::Dropped => ("⊗", "38;5;240", "Dropped"),
    }
}

pub fn status_mark(style: Style, status: Status, waiting: bool) -> String {
    let (shape, color, _) = mark(status, waiting);
    style.paint(color, shape)
}

pub fn status_label(style: Style, status: Status, waiting: bool) -> String {
    let (shape, color, name) = mark(status, waiting);
    style.paint(color, &format!("{shape} {name}"))
}

pub fn width(text: &str) -> usize {
    text.chars().count()
}

pub fn pad(text: &str, to: usize) -> String {
    let mut out = text.to_string();
    out.extend(std::iter::repeat_n(' ', to.saturating_sub(width(text))));
    out
}

pub fn cut(text: &str, to: usize) -> String {
    if width(text) <= to {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(to.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// `2026-11-25T10:00:12.345Z` as `2026-11-25 10:00`, for reading.
pub fn short_time(at: &str) -> String {
    at.get(..16)
        .map(|t| t.replacen('T', " ", 1))
        .unwrap_or_else(|| at.to_string())
}

pub fn id_text(id: TaskId) -> String {
    format!("#{}", id.0)
}

pub fn right(text: &str, to: usize) -> String {
    format!("{}{text}", " ".repeat(to.saturating_sub(width(text))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_only_on_a_terminal_without_no_color() {
        assert!(Style::decide(true, None).color);
        assert!(Style::decide(true, Some("")).color);
        assert!(!Style::decide(true, Some("1")).color);
        assert!(!Style::decide(false, None).color);
    }

    #[test]
    fn plain_style_writes_no_escape_codes() {
        let plain = Style { color: false };
        assert_eq!(status_mark(plain, Status::Doing, false), "◑");
        let color = Style { color: true };
        assert_eq!(
            status_mark(color, Status::Doing, false),
            "\x1b[38;5;214m◑\x1b[0m"
        );
    }

    #[test]
    fn every_status_has_its_own_mark_and_name() {
        let all = [
            (Status::Todo, false),
            (Status::Todo, true),
            (Status::Proposed, false),
            (Status::Doing, false),
            (Status::InReview, false),
            (Status::Blocked, false),
            (Status::Done, false),
            (Status::Dropped, false),
        ];
        let names: std::collections::HashSet<_> = all.iter().map(|(s, w)| mark(*s, *w).2).collect();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn long_titles_are_cut_with_an_ellipsis() {
        assert_eq!(cut("abcdef", 4), "abc…");
        assert_eq!(cut("abc", 4), "abc");
    }

    #[test]
    fn times_read_as_date_and_minute() {
        assert_eq!(short_time("2026-11-25T10:00:12.345Z"), "2026-11-25 10:00");
    }
}
