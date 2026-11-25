//! The `me` commands: what they do, and how they print.
//!
//! Text is for scanning: aligned columns and DESIGN.md's status marks, coloured only on a
//! terminal and never with `NO_COLOR`. `--json` prints the same shapes the tools return.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;

use std::path::Path;

use airboss_core::{
    Actor, Error, Event, EventId, EventKind, LinkKind, NewLink, NewTask, Repo, Status, Store,
    TaskEdit, TaskId, TaskNode,
};
use serde::Serialize;

use crate::render::{
    EventView, LinkView, NodeView, RepoNames, Style, Subject, TaskDetail, TaskList, TaskRef,
    TaskView, ToolError, cut, explain, id_text, mark, pad, right, short_time, status_label,
    status_mark, width,
};
use crate::tools::{self, GetTask};

/// The widest a title column gets before titles are cut with "…".
const TITLE_WIDTH: usize = 56;

/// How many events `airboss log` shows when not given `--after`.
pub const LOG_DEFAULT: usize = 50;

/// One row of the tree, before it's laid out.
struct Row {
    id: TaskId,
    depth: u32,
    status: Status,
    waiting: bool,
    title: String,
    repos: String,
    pill: String,
}

/// Lays rows out in aligned columns: id, indented mark and title, repos, then a pill.
fn table(rows: &[Row], style: Style) -> String {
    let id_w = rows
        .iter()
        .map(|r| width(&id_text(r.id)))
        .max()
        .unwrap_or(0);
    let title_w = rows
        .iter()
        .map(|r| r.depth as usize * 2 + width(&r.title))
        .max()
        .unwrap_or(0)
        .min(TITLE_WIDTH);
    let repo_w = rows.iter().map(|r| width(&r.repos)).max().unwrap_or(0);
    let mut out = String::new();
    for r in rows {
        let indent = "  ".repeat(r.depth as usize);
        let room = title_w
            .checked_sub(indent.len())
            .filter(|r| *r > 0)
            .unwrap_or(8);
        let title = cut(&r.title, room);
        let shown = match r.status {
            Status::Done | Status::Dropped => style.dim(&title),
            _ => title.clone(),
        };
        let mut line = format!(
            "{}  {indent}{} {shown}{}",
            style.dim(&right(&id_text(r.id), id_w)),
            status_mark(style, r.status, r.waiting),
            " ".repeat(room.saturating_sub(width(&title))),
        );
        if repo_w > 0 || !r.pill.is_empty() {
            let _ = write!(line, "  {}", pad(&r.repos, repo_w));
        }
        if !r.pill.is_empty() {
            let _ = write!(line, "  {}", r.pill);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The tree as `airboss ls` prints it. Done and dropped tasks are left out unless `all`,
/// except where an open task sits under them, so the tree keeps its shape.
pub fn tree_rows(nodes: &[TaskNode], all: bool) -> Vec<&TaskNode> {
    let closed = |n: &TaskNode| matches!(n.task.status, Status::Done | Status::Dropped);
    let mut keep: Vec<bool> = nodes.iter().map(|n| all || !closed(n)).collect();
    let index: HashMap<TaskId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.task.id, i))
        .collect();
    // Children come after their parents, so a backwards pass carries "keep" upwards.
    for i in (0..nodes.len()).rev() {
        if keep[i]
            && let Some(p) = nodes[i].task.parent.and_then(|p| index.get(&p))
        {
            keep[*p] = true;
        }
    }
    nodes
        .iter()
        .zip(keep)
        .filter_map(|(n, k)| k.then_some(n))
        .collect()
}

fn pill(style: Style, status: Status, waiting: bool, blocked_reason: Option<&str>) -> String {
    match status {
        Status::Blocked => style.paint(
            mark(status, waiting).1,
            &format!("blocked: {}", blocked_reason.unwrap_or("no reason given")),
        ),
        Status::Proposed => style.dim("proposed, waiting for you"),
        Status::Todo if waiting => style.dim("waiting"),
        _ => String::new(),
    }
}

/// `airboss ls`.
pub fn ls(
    store: &Store,
    all: bool,
    ready: bool,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let core = ToolError::from_core;
    let names = RepoNames::load(store)?;
    let nodes = store.tree().map_err(core)?;
    let (shown, since): (Vec<&TaskNode>, HashMap<TaskId, String>) = if ready {
        let ready = store.ready().map_err(core)?;
        let since: HashMap<TaskId, String> =
            ready.into_iter().map(|r| (r.task.id, r.since)).collect();
        (
            nodes
                .iter()
                .filter(|n| since.contains_key(&n.task.id))
                .collect(),
            since,
        )
    } else {
        (tree_rows(&nodes, all), HashMap::new())
    };
    if json {
        let tasks: Vec<NodeView> = shown
            .iter()
            .map(|n| NodeView {
                id: n.task.id,
                title: n.task.title.clone(),
                status: n.task.status,
                parent: n.task.parent,
                depth: n.depth,
                waiting: n.waiting,
                repos: names.names(&n.task.repos),
            })
            .collect();
        return to_json(&TaskList {
            total: tasks.len(),
            truncated: false,
            tasks,
        });
    }
    if shown.is_empty() {
        let hidden = !all && !nodes.is_empty();
        return Ok(match (ready, hidden) {
            (true, _) => "Nothing is ready to start.\n".into(),
            (false, true) => {
                "No open tasks. `airboss ls --all` shows done and dropped ones too.\n".into()
            }
            (false, false) => "No tasks yet. Add one with: airboss add \"What to do\"\n".into(),
        });
    }
    let rows: Vec<Row> = shown
        .iter()
        .map(|n| Row {
            id: n.task.id,
            depth: if ready { 0 } else { n.depth },
            status: n.task.status,
            waiting: n.waiting,
            title: n.task.title.clone(),
            repos: names.names(&n.task.repos).join(", "),
            pill: match since.get(&n.task.id) {
                Some(at) => style.dim(&format!("ready since {}", short_time(at))),
                None => pill(
                    style,
                    n.task.status,
                    n.waiting,
                    n.task.blocked_reason.as_deref(),
                ),
            },
        })
        .collect();
    Ok(table(&rows, style))
}

/// `airboss show`.
pub fn show(store: &Store, id: i64, json: bool, style: Style) -> Result<String, ToolError> {
    let detail = tools::get_task(store, GetTask { id })?;
    if json {
        return to_json(&detail);
    }
    Ok(detail_text(store, &detail, style))
}

const LABEL: usize = 10;

fn field(out: &mut String, style: Style, label: &str, lines: &[String]) {
    for (i, line) in lines.iter().enumerate() {
        let head = if i == 0 { label } else { "" };
        let _ = writeln!(out, "{}{line}", style.dim(&pad(head, LABEL)));
    }
}

fn task_ref(style: Style, t: &TaskRef) -> String {
    format!(
        "{} {} {}",
        status_mark(style, t.status, false),
        id_text(t.id),
        t.title
    )
}

fn detail_text(store: &Store, d: &TaskDetail, style: Style) -> String {
    let t = &d.task;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {}",
        style.dim(&id_text(t.id)),
        style.bold(&t.title)
    );
    let origin = match t.origin {
        airboss_core::Origin::Me => "you",
        airboss_core::Origin::Agent => "an agent",
        airboss_core::Origin::Plan => "a plan",
    };
    field(
        &mut out,
        style,
        "Status",
        &[format!(
            "{}  {}",
            status_label(style, t.status, d.waiting),
            style.dim(&format!("added by {origin}, {}", short_time(&t.created_at)))
        )],
    );
    if let Some(reason) = &t.blocked_reason {
        field(&mut out, style, "Why", std::slice::from_ref(reason));
    }
    if let Some(p) = t.parent {
        let title = store.task(p).map(|p| p.title).unwrap_or_default();
        field(
            &mut out,
            style,
            "Parent",
            &[format!("{} {title}", id_text(p))],
        );
    }
    if !t.repos.is_empty() {
        field(&mut out, style, "Repos", &[t.repos.join(", ")]);
    }
    if let Some(b) = &t.branch {
        field(&mut out, style, "Branch", std::slice::from_ref(b));
    }
    if let Some(body) = &t.body {
        let lines: Vec<String> = body.lines().map(str::to_string).collect();
        field(&mut out, style, "Notes", &lines);
    }
    if !d.links.is_empty() {
        let kind_w = d
            .links
            .iter()
            .map(|l| width(l.kind.as_str()))
            .max()
            .unwrap_or(0);
        let lines: Vec<String> = d
            .links
            .iter()
            .map(|l| link_line(style, l, kind_w))
            .collect();
        field(&mut out, style, "Links", &lines);
    }
    let refs = |v: &[TaskRef]| v.iter().map(|t| task_ref(style, t)).collect::<Vec<_>>();
    if !d.dependencies.waits_on.is_empty() {
        field(&mut out, style, "Waits on", &refs(&d.dependencies.waits_on));
    }
    if !d.dependencies.blocks.is_empty() {
        field(&mut out, style, "Blocks", &refs(&d.dependencies.blocks));
    }
    if !d.subtasks.is_empty() {
        field(&mut out, style, "Subtasks", &refs(&d.subtasks));
    }
    if let Some(order) = &d.run_order {
        let ids = |v: &[TaskRef]| {
            v.iter()
                .map(|t| id_text(t.id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut lines: Vec<String> = order
            .waves
            .iter()
            .enumerate()
            .map(|(i, w)| format!("Wave {}  {}", i + 1, ids(w)))
            .collect();
        for o in &order.waiting_outside {
            let on: Vec<String> = o.blockers.iter().map(|b| id_text(*b)).collect();
            lines.push(format!(
                "Waiting   {} on {}",
                id_text(o.task.id),
                on.join(", ")
            ));
        }
        if !order.tangled.is_empty() {
            lines.push(format!("Tangled   {}", ids(&order.tangled)));
        }
        if !lines.is_empty() {
            field(&mut out, style, "Run order", &lines);
        }
    }
    if !d.history.is_empty() {
        let label = if d.history_total > d.history.len() {
            format!("History, last {} of {}", d.history.len(), d.history_total)
        } else {
            "History".to_string()
        };
        let _ = writeln!(out, "{}", style.dim(&label));
        let events: Vec<&EventView> = d.history.iter().collect();
        out.push_str(&events_table(&events, style, false));
    }
    out
}

fn link_line(style: Style, l: &LinkView, kind_w: usize) -> String {
    let mut line = format!(
        "{}  {}",
        pad(l.kind.as_str(), kind_w),
        l.title.as_deref().unwrap_or(&l.r#ref)
    );
    if l.title.is_some() {
        let _ = write!(line, "  {}", style.dim(&l.r#ref));
    }
    if let Some(state) = l.pr_state {
        let _ = write!(line, "  {state}");
    }
    if let Some(url) = &l.url {
        let _ = write!(line, "  {}", style.dim(url));
    }
    line
}

/// What an event did, in a few words.
fn what(e: &EventView) -> String {
    let word = match (e.kind, e.to_status) {
        (EventKind::Status, Some(to)) => return format!("→ {}", mark(to, false).2.to_lowercase()),
        (EventKind::Status, None) => "status",
        (EventKind::Created, _) => "created",
        (EventKind::Edited, _) => "edited",
        (EventKind::Moved, _) => "moved",
        (EventKind::Note, _) => "note",
        (EventKind::Linked, _) => "linked",
        (EventKind::Unlinked, _) => "unlinked",
        (EventKind::LinkState, _) => "PR state",
        (EventKind::WaitsOnAdded, _) => "waits on",
        (EventKind::WaitsOnRemoved, _) => "stopped waiting",
        (EventKind::Rejected, _) => "rejected",
        (EventKind::RepoLinked, _) => "repo linked",
        (EventKind::RepoUnlinked, _) => "repo unlinked",
    };
    word.to_string()
}

/// An event's body, without a lead-in that repeats [`what`], e.g. "waits on #2".
fn body_after<'a>(what: &str, body: &'a str) -> &'a str {
    body.strip_prefix(what).map(str::trim_start).unwrap_or(body)
}

fn events_table(events: &[&EventView], style: Style, with_task: bool) -> String {
    let actor_w = events
        .iter()
        .map(|e| width(&e.actor.to_string()))
        .max()
        .unwrap_or(0);
    let what_w = events.iter().map(|e| width(&what(e))).max().unwrap_or(0);
    let task_w = events
        .iter()
        .map(|e| width(&id_text(e.task)))
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for e in events {
        let mut line = format!("  {}  ", style.dim(&short_time(&e.at)));
        if with_task {
            let _ = write!(line, "{}  ", right(&id_text(e.task), task_w));
        }
        let _ = write!(
            line,
            "{}  {}",
            pad(&e.actor.to_string(), actor_w),
            pad(&what(e), what_w)
        );
        if let Some(body) = &e.body {
            let first = body.lines().next().unwrap_or_default();
            let _ = write!(line, "  {}", body_after(&what(e), first));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// `airboss log`: the newest events, oldest first, or everything after an event id.
pub fn log(
    store: &Store,
    after: Option<i64>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let all: Vec<Event> = store
        .events_after(EventId(after.unwrap_or(0)))
        .map_err(ToolError::from_core)?;
    let skip = if after.is_some() {
        0
    } else {
        all.len().saturating_sub(LOG_DEFAULT)
    };
    let events: Vec<EventView> = all.into_iter().skip(skip).map(EventView::from).collect();
    if json {
        return to_json(&events);
    }
    if events.is_empty() {
        return Ok("Nothing has happened yet.\n".into());
    }
    let refs: Vec<&EventView> = events.iter().collect();
    Ok(events_table(&refs, style, true))
}

/// `airboss repo ls`.
pub fn repo_ls(store: &Store, json: bool, style: Style) -> Result<String, ToolError> {
    let repos: Vec<Repo> = store.repos().map_err(ToolError::from_core)?;
    if json {
        return to_json(&repos);
    }
    if repos.is_empty() {
        return Ok("No repos yet. Add one with: airboss repo add <name> <path>\n".into());
    }
    let name_w = repos.iter().map(|r| width(&r.name)).max().unwrap_or(0);
    let path_w = repos.iter().map(|r| width(&r.path)).max().unwrap_or(0);
    let mut out = String::new();
    for r in &repos {
        let line = format!(
            "{}  {}  {}",
            style.bold(&pad(&r.name, name_w)),
            pad(&r.path, path_w),
            style.dim(r.remote_url.as_deref().unwrap_or(""))
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    Ok(out)
}

// ---- writes, all as `me` ----

const ME: &Actor = &Actor::Me;

fn fail(store: &Store, about: Subject) -> impl Fn(airboss_core::Error) -> ToolError + '_ {
    move |e| explain(store, ME, e, &about)
}

fn view(store: &Store, id: TaskId) -> Result<TaskView, ToolError> {
    let task = store.task(id).map_err(fail(store, Subject::task(id)))?;
    Ok(TaskView::new(task, &RepoNames::load(store)?))
}

/// `○ #12 Fix booking typo`.
fn task_line(style: Style, t: &TaskView) -> String {
    format!(
        "{} {} {}",
        status_mark(style, t.status, false),
        id_text(t.id),
        t.title
    )
}

/// Prints a changed task as JSON, or as `text` around its one-line summary.
fn done_with(
    t: &TaskView,
    json: bool,
    text: impl FnOnce(&TaskView) -> String,
) -> Result<String, ToolError> {
    if json {
        to_json(t)
    } else {
        Ok(format!("{}\n", text(t)))
    }
}

/// What `airboss add` takes.
pub struct Add<'a> {
    pub title: &'a str,
    pub under: Option<i64>,
    pub repos: &'a [String],
    pub body: Option<&'a str>,
}

/// `airboss add`: a new task, `todo`, at the end of its siblings.
pub fn add(
    store: &mut Store,
    args: Add<'_>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let repos = store.repos().map_err(ToolError::from_core)?;
    let mut new = NewTask::new(args.title);
    new.parent = args.under.map(TaskId);
    new.body = args
        .body
        .filter(|b| !b.trim().is_empty())
        .map(str::to_string);
    for id in tools::repo_ids(&repos, args.repos)? {
        new = new.in_repo(id);
    }
    let about = Subject {
        parent: new.parent,
        ..Subject::default()
    };
    let task = store.add_task(new, ME).map_err(fail(store, about))?;
    let t = TaskView::new(task, &RepoNames::new(&repos));
    done_with(&t, json, |t| {
        let mut line = format!("Added {}", task_line(style, t));
        if let Some(p) = t.parent {
            let _ = write!(line, " under {}", id_text(p));
        }
        if !t.repos.is_empty() {
            let _ = write!(line, " in {}", t.repos.join(", "));
        }
        line
    })
}

/// A status `airboss set` takes; done and dropped have their own commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SetStatus {
    /// Not started.
    Todo,
    /// In progress.
    Doing,
    /// Waiting for review.
    #[value(name = "in_review", alias = "in-review")]
    InReview,
    /// Stuck; give --note saying why.
    Blocked,
}

impl From<SetStatus> for Status {
    fn from(s: SetStatus) -> Status {
        match s {
            SetStatus::Todo => Status::Todo,
            SetStatus::Doing => Status::Doing,
            SetStatus::InReview => Status::InReview,
            SetStatus::Blocked => Status::Blocked,
        }
    }
}

/// `airboss set`, `done` and `drop`. Dropping drops the open tasks under it too.
pub fn set(
    store: &mut Store,
    id: i64,
    to: Status,
    note: Option<&str>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    let open_under = if to == Status::Dropped {
        open_subtasks(store, id)?
    } else {
        0
    };
    let about = Subject {
        to: Some(to),
        ..Subject::task(id)
    };
    store
        .set_status(id, to, note, ME)
        .map_err(fail(store, about))?;
    let t = view(store, id)?;
    done_with(&t, json, |t| {
        let name = mark(t.status, false).2.to_lowercase();
        let mut line = format!("{} is now {name}", task_line(style, t));
        if open_under > 0 {
            let s = if open_under == 1 { "" } else { "s" };
            let _ = write!(line, ", with {open_under} open subtask{s}");
        }
        line
    })
}

fn open_subtasks(store: &Store, id: TaskId) -> Result<usize, ToolError> {
    let nodes = store.tree().map_err(ToolError::from_core)?;
    let Some(start) = nodes.iter().position(|n| n.task.id == id) else {
        return Ok(0);
    };
    let depth = nodes[start].depth;
    Ok(nodes[start + 1..]
        .iter()
        .take_while(|n| n.depth > depth)
        .filter(|n| !matches!(n.task.status, Status::Done | Status::Dropped))
        .count())
}

/// `airboss accept` and `reject`: only for an agent's proposal.
pub fn decide(
    store: &mut Store,
    id: i64,
    accept: bool,
    note: Option<&str>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    let before = view(store, id)?;
    if before.status != Status::Proposed {
        return Err(ToolError::new(format!(
            "Task #{} isn't a proposal; it's {}. Use airboss set, done or drop instead.",
            id.0, before.status
        )));
    }
    let to = if accept {
        Status::Todo
    } else {
        Status::Dropped
    };
    store.set_status(id, to, note, ME).map_err(fail(
        store,
        Subject {
            to: Some(to),
            ..Subject::task(id)
        },
    ))?;
    let t = view(store, id)?;
    done_with(&t, json, |t| {
        if accept {
            format!("Accepted {}; it's todo now", task_line(style, t))
        } else {
            format!(
                "Rejected {}; it's dropped and hidden from the tree",
                task_line(style, t)
            )
        }
    })
}

/// `airboss edit`: the title, the notes, or both. An empty `--body` clears the notes.
pub fn edit(
    store: &mut Store,
    id: i64,
    title: Option<&str>,
    body: Option<&str>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    if title.is_none() && body.is_none() {
        return Err(ToolError::new("Give --title, --body, or both."));
    }
    let mut change = TaskEdit::default();
    if let Some(t) = title {
        change = change.title(t);
    }
    if let Some(b) = body {
        change = change.body(Some(b).filter(|b| !b.trim().is_empty()));
    }
    store
        .edit_task(id, change, ME)
        .map_err(fail(store, Subject::task(id)))?;
    let t = view(store, id)?;
    done_with(&t, json, |t| format!("Edited {}", task_line(style, t)))
}

/// Where `airboss mv` puts a task.
pub enum Target {
    /// Under this task.
    Under(i64),
    /// At the top level.
    Top,
    /// Where it is, only reordered.
    Same,
}

/// `airboss mv`: a new parent, a new position, or both. Left out, the position is the end.
pub fn mv(
    store: &mut Store,
    id: i64,
    target: Target,
    pos: Option<u32>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    let current = view(store, id)?;
    let parent = match target {
        Target::Under(p) => Some(TaskId(p)),
        Target::Top => None,
        Target::Same => current.parent,
    };
    let about = Subject {
        parent,
        ..Subject::task(id)
    };
    store
        .move_task(id, parent, pos.unwrap_or(u32::MAX), ME)
        .map_err(fail(store, about))?;
    let t = view(store, id)?;
    let position = store.task(id).map(|t| t.position).unwrap_or_default();
    done_with(&t, json, |t| {
        let place = match t.parent {
            Some(p) => format!("under {}", id_text(p)),
            None => "at the top level".into(),
        };
        format!("Moved {} {place}, position {position}", task_line(style, t))
    })
}

/// `airboss wait` and `unwait`.
pub fn wait(
    store: &mut Store,
    id: i64,
    on: i64,
    add: bool,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let (id, on) = (TaskId(id), TaskId(on));
    let about = Subject {
        others: vec![on],
        ..Subject::task(id)
    };
    let done = if add {
        store.add_dependency(id, on, ME)
    } else {
        store.remove_dependency(id, on, ME)
    };
    done.map_err(|e| match e {
        airboss_core::Error::NotFound
            if !add && store.task(id).is_ok() && store.task(on).is_ok() =>
        {
            ToolError::new(format!("Task #{} doesn't wait on #{}.", id.0, on.0))
        }
        e => explain(store, ME, e, &about),
    })?;
    let t = view(store, id)?;
    let blocker = view(store, on)?;
    done_with(&t, json, |t| {
        let verb = if add {
            "now waits on"
        } else {
            "no longer waits on"
        };
        format!(
            "{} {verb} {}",
            task_line(style, t),
            task_line(style, &blocker)
        )
    })
}

/// `airboss link`.
pub fn link(
    store: &mut Store,
    id: i64,
    new: NewLink,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    let is_pr = new.kind == LinkKind::Pr;
    let link = store.link(id, new, ME).map_err(|e| match e {
        Error::InvalidTarget if is_pr => ToolError::bad_pr_ref(),
        e => explain(store, ME, e, &Subject::task(id)),
    })?;
    if json {
        return to_json(&LinkView::from(link));
    }
    let t = view(store, id)?;
    Ok(format!(
        "Linked {} {} to {}\n",
        link.kind,
        link.r#ref,
        task_line(style, &t)
    ))
}

/// `airboss repo add`. A relative path is taken from the current directory.
pub fn repo_add(
    store: &mut Store,
    name: &str,
    path: &Path,
    remote: Option<&str>,
    json: bool,
) -> Result<String, ToolError> {
    let path = path
        .canonicalize()
        .or_else(|_| std::env::current_dir().map(|d| d.join(path)))
        .map_err(|e| ToolError::new(format!("Couldn't resolve {}: {e}", path.display())))?;
    let repo = store
        .add_repo(name.trim(), &path, remote, ME)
        .map_err(ToolError::from_core)?;
    if json {
        return to_json(&repo);
    }
    Ok(format!("Added repo {} at {}\n", repo.name, repo.path))
}

/// `airboss repo link` and `repo unlink`.
pub fn repo_link(
    store: &mut Store,
    id: i64,
    name: &str,
    add: bool,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    let repos = store.repos().map_err(ToolError::from_core)?;
    let repo = tools::repo_ids(&repos, &[name.to_string()])?[0];
    let about = Subject {
        repo: Some(name.trim().to_string()),
        ..Subject::task(id)
    };
    let done = if add {
        store.link_repo(id, repo, ME)
    } else {
        store.unlink_repo(id, repo, ME)
    };
    done.map_err(fail(store, about))?;
    let t = view(store, id)?;
    done_with(&t, json, |t| match t.repos.as_slice() {
        [] => format!("{} has no repos now", task_line(style, t)),
        names => format!("{} is linked to {}", task_line(style, t), names.join(", ")),
    })
}

/// `airboss branch`: sets the branch every thread for the task shares, or clears it.
pub fn branch(
    store: &mut Store,
    id: i64,
    name: Option<&str>,
    json: bool,
    style: Style,
) -> Result<String, ToolError> {
    let id = TaskId(id);
    let task = store.set_branch(id, name, ME).map_err(|e| match e {
        Error::InvalidTarget => ToolError::bad_branch(),
        e => explain(store, ME, e, &Subject::task(id)),
    })?;
    let t = TaskView::new(task, &RepoNames::load(store)?);
    done_with(&t, json, |t| match &t.branch {
        Some(b) => format!("{} is on branch {b}", task_line(style, t)),
        None => format!("{} has no branch now", task_line(style, t)),
    })
}

pub fn to_json<T: Serialize>(value: &T) -> Result<String, ToolError> {
    serde_json::to_string_pretty(value)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| ToolError::new(format!("Couldn't encode the result: {e}")))
}

/// Writes command output to stdout. A closed pipe (`airboss ls | head`) isn't an error.
pub fn print(text: &str) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(format!("couldn't write output: {e}"))
        }
        _ => Ok(()),
    }
}
