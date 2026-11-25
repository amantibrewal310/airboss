//! One function per kind (Quiet is in `quiet.rs`). Each adds its entries, unsorted and
//! unhidden; `compute` orders and hides them.

use std::collections::{BTreeMap, HashMap, HashSet};

use airboss_core::{PrState, Status, TaskId, TaskNode};
use airboss_services::time::from_ts;
use airboss_services::{
    Checks, NeedsKind, PrView, RequestKind, Review, Target, ThreadState, ThreadView,
};

use crate::compute::{Ctx, Entry, ItemFor, count, is_open, stamp};

/// Questions, approvals and failed runs, from linked threads.
pub(crate) fn threads(ctx: &Ctx<'_>, out: &mut Vec<Entry>) {
    for (key, tasks) in &ctx.sync.thread_tasks {
        let Some(thread) = ctx.sync.threads.get(key).filter(|t| t.linked) else {
            continue;
        };
        if !matches!(thread.state, ThreadState::NeedsMe | ThreadState::Errored) {
            continue;
        }
        let tasks = ctx.open_tasks(tasks);
        let Some(node) = tasks.first().and_then(|&t| ctx.node(t)) else {
            continue;
        };
        let Some(item) = thread_item(thread, node) else {
            continue;
        };
        out.push(Entry {
            item: item.build(thread.stale),
            tasks,
            changes_at: None,
        });
    }
}

fn thread_item<'a>(thread: &ThreadView, node: &'a TaskNode) -> Option<ItemFor<'a>> {
    let name = thread
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(&node.task.title);
    let target = Target::Thread(thread.key.clone());
    match (thread.state, &thread.request) {
        (ThreadState::NeedsMe, Some(req)) => {
            let (kind, headline, context) = match req.kind {
                RequestKind::Question => (
                    NeedsKind::Question,
                    req.text.trim().to_string(),
                    format!("Agent in {name} asks"),
                ),
                RequestKind::Approval => (
                    NeedsKind::Approval,
                    format!("Allow: {}", first_line(&req.text)),
                    format!("Agent in {name} wants approval"),
                ),
            };
            let prefix = match kind {
                NeedsKind::Question => "question",
                _ => "approval",
            };
            Some(ItemFor {
                kind,
                key: format!("{prefix}:{}", req.id),
                node,
                since: stamp(&req.at),
                headline,
                context,
                target,
            })
        }
        (ThreadState::Errored, _) => {
            let last = thread.last_activity_at.as_deref();
            let key = match &thread.run_id {
                Some(run) => format!("failed:{run}"),
                None => format!("failed:{}@{}", thread.key.r#ref, last.unwrap_or("")),
            };
            let error = thread
                .error
                .as_deref()
                .map(first_line)
                .filter(|e| !e.is_empty());
            let (headline, context) = match error {
                Some(e) => (
                    format!("{name} stopped: {e}"),
                    format!("Thread stopped · {e}"),
                ),
                None => (format!("{name} stopped"), "Thread stopped".to_string()),
            };
            Some(ItemFor {
                kind: NeedsKind::FailedRun,
                key,
                node,
                since: stamp(last.unwrap_or(&node.last_event_at)),
                headline,
                context,
                target,
            })
        }
        _ => None,
    }
}

/// The first non-blank line, trimmed.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// Linked PRs that are open, not drafts and readable, on an open task, with no agent
/// running on or waiting in any of the PR's open tasks.
pub(crate) fn reviews(ctx: &Ctx<'_>, out: &mut Vec<Entry>) {
    for (pr_ref, tasks) in &ctx.sync.pr_tasks {
        let Some(pr) = ctx.sync.prs.get(pr_ref) else {
            continue;
        };
        if pr.state != Some(PrState::Open) || pr.draft || pr.problem.is_some() {
            continue;
        }
        let tasks = ctx.open_tasks(tasks);
        if tasks.iter().any(|t| ctx.agent_on.contains(t)) {
            continue;
        }
        let Some(node) = tasks.first().and_then(|&t| ctx.node(t)) else {
            continue;
        };
        let (_, _, number) = pr_ref.parts();
        let headline = match pr.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(title) => format!("PR #{number} · {title}"),
            None => format!("PR #{number}"),
        };
        let since = pr
            .ready_since
            .as_deref()
            .or(pr.updated_at.as_deref())
            .unwrap_or(&node.last_event_at);
        let item = ItemFor {
            kind: NeedsKind::Review,
            key: format!("review:{pr_ref}"),
            node,
            since: stamp(since),
            headline,
            context: review_context(pr),
            target: Target::Pr {
                r#ref: pr_ref.clone(),
                url: pr.url.clone(),
            },
        };
        out.push(Entry {
            item: item.build(pr.stale),
            tasks,
            changes_at: None,
        });
    }
}

/// "+{add} −{del} · checks {…} · {review}", leaving out what GitHub didn't say.
fn review_context(pr: &PrView) -> String {
    let mut parts = Vec::with_capacity(3);
    if let (Some(add), Some(del)) = (pr.additions, pr.deletions) {
        parts.push(format!("+{add} \u{2212}{del}"));
    }
    match pr.checks {
        Checks::Passing => parts.push("checks passing".to_string()),
        Checks::Failing => parts.push("checks failing".to_string()),
        Checks::Pending => parts.push("checks pending".to_string()),
        Checks::None => {}
    }
    match pr.review {
        Review::Approved => parts.push("approved".to_string()),
        Review::ChangesRequested => parts.push("changes requested".to_string()),
        Review::Required => parts.push("review required".to_string()),
        Review::None => {}
    }
    match parts.is_empty() {
        true => "Ready for review".to_string(),
        false => parts.join(" · "),
    }
}

/// Open proposals: one item per parent, and each top-level proposal on its own.
pub(crate) fn proposed(ctx: &Ctx<'_>, out: &mut Vec<Entry>) {
    // By parent, or by the proposal itself when it's top-level (`true`).
    let mut groups: BTreeMap<(TaskId, bool), Vec<&TaskNode>> = BTreeMap::new();
    for node in &ctx.core.tree {
        if node.task.status == Status::Proposed {
            let owner = node
                .task
                .parent
                .map_or((node.task.id, true), |p| (p, false));
            groups.entry(owner).or_default().push(node);
        }
    }
    for ((owner, top_level), mut nodes) in groups {
        let Some(owner_node) = ctx.node(owner) else {
            continue;
        };
        nodes.sort_by_key(|n| n.task.id);
        let n = nodes.len();
        let newest = nodes.iter().map(|n| n.task.created_at.as_str()).max();
        let (key, headline, context) = match top_level {
            true => (
                format!("proposed:{}", owner.0),
                "The agent proposed a task".to_string(),
                "Proposed task".to_string(),
            ),
            false => {
                let largest = nodes.last().map_or(owner.0, |n| n.task.id.0);
                (
                    format!("proposed:{}:{largest}", owner.0),
                    format!("The agent proposed {}", count(n, "subtask")),
                    format!("{} proposed {}", n, plural(n, "subtask")),
                )
            }
        };
        let mut item = ItemFor {
            kind: NeedsKind::Proposed,
            key,
            node: owner_node,
            since: stamp(newest.unwrap_or(&owner_node.task.created_at)),
            headline,
            context,
            target: Target::Task(owner),
        }
        .build(false);
        item.proposed = nodes
            .iter()
            .map(|n| (n.task.id, n.task.title.clone()))
            .collect();
        out.push(Entry {
            item,
            tasks: vec![owner],
            changes_at: None,
        });
    }
}

/// Tasks in `ready` that waited on something, while `since` is inside the window. Each
/// leaves the list when its window ends.
pub(crate) fn ready(ctx: &Ctx<'_>, out: &mut Vec<Entry>) {
    if ctx.core.ready.is_empty() {
        return;
    }
    // `edges` are sorted by task, then blocker, so each list is in id order.
    let mut blockers: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
    for &(task, on) in &ctx.core.edges {
        blockers.entry(task).or_default().push(on);
    }
    let merged: HashSet<TaskId> = ctx
        .sync
        .pr_tasks
        .iter()
        .filter(|(pr, _)| {
            ctx.sync
                .prs
                .get(*pr)
                .is_some_and(|p| p.state == Some(PrState::Merged))
        })
        .flat_map(|(_, tasks)| tasks.iter().copied())
        .collect();
    let window_start = ctx.cutoff(ctx.cfg.ready_window);
    for r in &ctx.core.ready {
        let Some(on) = blockers.get(&r.task.id) else {
            continue;
        };
        let since = stamp(&r.since);
        if since.as_str() <= window_start.as_str() {
            continue;
        }
        let Some(node) = ctx.node(r.task.id) else {
            continue;
        };
        let (headline, context) = ready_text(on, &merged);
        let item = ItemFor {
            kind: NeedsKind::Ready,
            key: format!("ready:{}:{since}", r.task.id.0),
            node,
            since: since.clone(),
            headline,
            context,
            target: Target::Task(r.task.id),
        };
        out.push(Entry {
            item: item.build(false),
            tasks: vec![r.task.id],
            changes_at: from_ts(&since).and_then(|t| t.checked_add(ctx.cfg.ready_window)),
        });
    }
}

/// "Ready to start: 143 and 145 are done" · "143 merged and 145 done · ready to start".
fn ready_text(on: &[TaskId], merged: &HashSet<TaskId>) -> (String, String) {
    if on.len() > 2 {
        return (
            format!("Ready to start: {} tasks it waited on are done", on.len()),
            format!("{} tasks done · ready to start", on.len()),
        );
    }
    let ids: Vec<String> = on.iter().map(|t| t.0.to_string()).collect();
    let verb = match on.len() {
        1 => "is",
        _ => "are",
    };
    let headline = format!("Ready to start: {} {verb} done", ids.join(" and "));
    let done = |t: &TaskId| match merged.contains(t) {
        true => "merged",
        false => "done",
    };
    let context = match on {
        [a, b] if done(a) != done(b) => format!("{} {} and {} {}", a.0, done(a), b.0, done(b)),
        _ => format!("{} {}", ids.join(" and "), on.first().map_or("done", done)),
    };
    (headline, format!("{context} · ready to start"))
}

/// Open parents whose direct subtasks that aren't dropped are all done, with at least one.
/// A proposed subtask counts as not done.
pub(crate) fn finish(ctx: &Ctx<'_>, out: &mut Vec<Entry>) {
    // Per parent position: kept subtasks, done ones, the newest done one's last event.
    let mut kids: Vec<(usize, usize, Option<&str>)> = vec![(0, 0, None); ctx.core.tree.len()];
    for node in &ctx.core.tree {
        let Some(&p) = node.task.parent.and_then(|p| ctx.index.get(&p)) else {
            continue;
        };
        let k = &mut kids[p];
        match node.task.status {
            Status::Dropped => {}
            Status::Done => {
                k.0 += 1;
                k.1 += 1;
                let at = node.last_event_at.as_str();
                if k.2.is_none_or(|newest| at > newest) {
                    k.2 = Some(at);
                }
            }
            _ => k.0 += 1,
        }
    }
    for (node, &(kept, done, newest)) in ctx.core.tree.iter().zip(&kids) {
        let Some(newest) = newest.filter(|_| kept > 0 && kept == done) else {
            continue;
        };
        if !is_open(node.task.status) {
            continue;
        }
        let since = stamp(newest);
        let (headline, context) = match done {
            1 => (
                "1 subtask is done. Mark the parent done?".to_string(),
                "1 subtask done".to_string(),
            ),
            n => (
                format!("All {n} subtasks are done. Mark the parent done?"),
                format!("All {n} subtasks done"),
            ),
        };
        let item = ItemFor {
            kind: NeedsKind::FinishIt,
            key: format!("finish:{}:{since}", node.task.id.0),
            node,
            since,
            headline,
            context,
            target: Target::Task(node.task.id),
        };
        out.push(Entry {
            item: item.build(false),
            tasks: vec![node.task.id],
            changes_at: None,
        });
    }
}

/// The noun, plural unless there's one.
fn plural(n: usize, noun: &str) -> String {
    match n {
        1 => noun.to_string(),
        _ => format!("{noun}s"),
    }
}
