//! Agent stopped: a `doing` task whose agents have all stopped, with no PR to review. Never a
//! parent, whose status follows its steps.
//!
//! A run that ends without a PR leaves its task `doing` with nothing to decide in any other
//! kind until it turns Quiet days later. So once every linked thread is idle, settled or gone
//! (an errored one is a Failed run instead), no PR on the task is open, and nothing on the task
//! or its threads has changed for `stopped_after` (10 minutes), the task asks to be finished.

use std::borrow::Cow;
use std::collections::HashMap;

use airboss_core::{PrState, Status, TaskId};
use airboss_services::time::from_ts;
use airboss_services::{NeedsKind, Target, ThreadState, ThreadView};

use crate::compute::{Ctx, Entry, ItemFor, Wake, canonical};

/// The task's PRs, as far as Agent stopped cares.
#[derive(Debug, Default, Clone, Copy)]
struct Prs {
    /// One is open (a draft too), or its state isn't known yet: the agent may have made one.
    open: bool,
    /// It has any PR at all, so the copy says "no open PR" rather than "no PR yet".
    any: bool,
}

pub(crate) fn stopped(ctx: &Ctx<'_>, out: &mut Vec<Entry>, wake: &mut Wake) {
    // Each `doing` task's linked threads; `None` for one sync has no view of yet. A parent
    // shows doing because a step is, and nothing finishes it (SPEC-core rule 10), so its
    // threads ask nothing here.
    let mut threads: HashMap<TaskId, Vec<(&str, Option<&ThreadView>)>> = HashMap::new();
    for (key, tasks) in &ctx.sync.thread_tasks {
        let view = ctx.sync.threads.get(key);
        for &task in tasks {
            if ctx
                .node(task)
                .is_some_and(|n| n.task.status == Status::Doing && !n.has_steps)
            {
                let list = threads.entry(task).or_default();
                if !list.iter().any(|(r, _)| *r == key.r#ref) {
                    list.push((&key.r#ref, view));
                }
            }
        }
    }
    if threads.is_empty() {
        return;
    }
    let mut prs: HashMap<TaskId, Prs> = HashMap::new();
    for (pr_ref, tasks) in &ctx.sync.pr_tasks {
        let open = ctx
            .sync
            .prs
            .get(pr_ref)
            .is_none_or(|p| !matches!(p.state, Some(PrState::Closed | PrState::Merged)));
        for &task in tasks {
            let p = prs.entry(task).or_default();
            p.any = true;
            p.open |= open;
        }
    }

    for (task, list) in threads {
        let Some(node) = ctx.node(task) else {
            continue;
        };
        let pr = prs.get(&task).copied().unwrap_or_default();
        if pr.open {
            continue;
        }
        // Every thread known and stopped.
        let Some(views) = list
            .iter()
            .map(|(_, v)| v.filter(|v| is_stopped(v.state)))
            .collect::<Option<Vec<&ThreadView>>>()
        else {
            continue;
        };
        // The newest change to the task itself (not its subtasks) or to any of its threads.
        let mut since: Cow<'_, str> =
            canonical(&node.last_event_at).unwrap_or(Cow::Borrowed(node.last_event_at.as_str()));
        let mut latest: Option<(&ThreadView, Cow<'_, str>)> = None;
        for &v in &views {
            let at = v.last_activity_at.as_deref().and_then(canonical);
            if let Some(at) = &at
                && *at > since
            {
                since = at.clone();
            }
            let at = at.unwrap_or(Cow::Borrowed(""));
            let newer = latest
                .as_ref()
                .is_none_or(|(l, lat)| (&at, &v.key.r#ref) > (lat, &l.key.r#ref));
            if newer {
                latest = Some((v, at));
            }
        }
        let Some((target, _)) = latest else {
            continue;
        };
        let Some(due) = from_ts(&since).and_then(|t| t.checked_add(ctx.cfg.stopped_after)) else {
            continue;
        };
        if ctx.now < due {
            wake.at(due);
            continue;
        }
        let context = match views.len() {
            1 => format!("{} stopped", name(target, &node.task.title)),
            n => format!("All {n} threads stopped"),
        };
        let headline = match pr.any {
            true => "Agent stopped · no open PR",
            false => "Agent stopped · no PR yet",
        };
        let item = ItemFor {
            kind: NeedsKind::AgentStopped,
            key: format!("stopped:{}:{since}", task.0),
            node,
            since: since.into_owned(),
            headline: headline.to_string(),
            context,
            target: Target::Thread(target.key.clone()),
        };
        out.push(Entry {
            item: item.build(views.iter().any(|v| v.stale)),
            tasks: vec![task],
            changes_at: None,
        });
    }
}

/// Idle, settled or gone. Running and waiting on me are still going; errored is a Failed run.
fn is_stopped(state: ThreadState) -> bool {
    matches!(
        state,
        ThreadState::Idle | ThreadState::Settled | ThreadState::Gone
    )
}

/// The thread's title, or its task's when it has none.
fn name<'a>(thread: &'a ThreadView, task_title: &'a str) -> &'a str {
    thread
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(task_title)
}
