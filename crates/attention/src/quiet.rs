//! Last activity per subtree, and Quiet.
//!
//! A task's last activity is the newest of `subtree_last_event_at` and, for it and every
//! task under it, each linked thread's `last_activity_at` (a `Running` or `NeedsMe` thread
//! counts as now) and each linked PR's `updated_at`. One bottom-up pass works that out;
//! Quiet is then decided top-down, so a quiet parent hides its quiet subtasks.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use airboss_core::{PrState, Status, TaskId};
use airboss_services::time::from_ts;
use airboss_services::{NeedsKind, Target};

use crate::compute::{Ctx, Entry, ItemFor, Wake, canonical, count, is_live};

const DAY: Duration = Duration::from_secs(86_400);

/// One subtree's last activity.
#[derive(Debug, Clone)]
struct Activity<'a> {
    /// The newest time, in `core`'s text form.
    at: Cow<'a, str>,
    /// An agent is running or waiting somewhere in it: active now.
    live: bool,
    /// Some thread or PR that fed it is stale.
    stale: bool,
}

impl<'a> Activity<'a> {
    fn saw(&mut self, at: Cow<'a, str>) {
        if at > self.at {
            self.at = at;
        }
    }

    fn merge(&mut self, child: &Activity<'a>) {
        self.live |= child.live;
        self.stale |= child.stale;
        if child.at > self.at {
            self.at = child.at.clone();
        }
    }
}

/// Doing, in review or blocked: work that can go quiet.
fn in_progress(status: Status) -> bool {
    matches!(status, Status::Doing | Status::InReview | Status::Blocked)
}

pub(crate) fn quiet<'a>(ctx: &Ctx<'a>, out: &mut Vec<Entry>, wake: &mut Wake) {
    let tree = &ctx.core.tree;
    let mut act: Vec<Activity<'a>> = tree
        .iter()
        .map(|n| Activity {
            at: Cow::Borrowed(n.subtree_last_event_at.as_str()),
            live: false,
            stale: false,
        })
        .collect();
    let positions = |tasks: &'a [TaskId]| tasks.iter().filter_map(|t| ctx.index.get(t).copied());

    for (key, tasks) in &ctx.sync.thread_tasks {
        let Some(thread) = ctx.sync.threads.get(key) else {
            continue;
        };
        let at = thread.last_activity_at.as_deref().and_then(canonical);
        for i in positions(tasks) {
            let a = &mut act[i];
            a.stale |= thread.stale;
            a.live |= is_live(thread.state);
            if let Some(at) = &at {
                a.saw(at.clone());
            }
        }
    }
    let mut open_prs: HashMap<TaskId, usize> = HashMap::new();
    for (pr_ref, tasks) in &ctx.sync.pr_tasks {
        let Some(pr) = ctx.sync.prs.get(pr_ref) else {
            continue;
        };
        if pr.state == Some(PrState::Open) {
            for &t in tasks {
                *open_prs.entry(t).or_default() += 1;
            }
        }
        let at = pr.updated_at.as_deref().and_then(canonical);
        for i in positions(tasks) {
            let a = &mut act[i];
            a.stale |= pr.stale;
            if let Some(at) = &at {
                a.saw(at.clone());
            }
        }
    }

    // Bottom-up: `tree` is depth-first, so walking it backwards reaches every child before
    // its parent, and a parent always sits at a lower position.
    let parent_of: Vec<Option<usize>> = tree
        .iter()
        .enumerate()
        .map(|(i, n)| {
            n.task
                .parent
                .and_then(|p| ctx.index.get(&p).copied())
                .filter(|&p| p < i)
        })
        .collect();
    for i in (0..tree.len()).rev() {
        if let Some(p) = parent_of[i] {
            let (before, from_i) = act.split_at_mut(i);
            before[p].merge(&from_i[0]);
        }
    }

    // Top-down: a task is skipped when an ancestor is already shown as quiet.
    let cutoff = ctx.cutoff(ctx.cfg.quiet_after);
    let mut quiet_above = vec![false; tree.len()];
    let mut is_quiet = vec![false; tree.len()];
    let mut soonest: Option<usize> = None;
    for (i, node) in tree.iter().enumerate() {
        let above = parent_of[i].is_some_and(|p| quiet_above[p] || is_quiet[p]);
        quiet_above[i] = above;
        let a = &act[i];
        if above || a.live || !in_progress(node.task.status) {
            continue;
        }
        if a.at.as_ref() > cutoff.as_str() {
            if soonest.is_none_or(|s| a.at < act[s].at) {
                soonest = Some(i);
            }
            continue;
        }
        is_quiet[i] = true;
        let (days, changes_at) = match from_ts(&a.at) {
            Some(t) => {
                let days = ctx.now.duration_since(t).unwrap_or_default().as_secs() / DAY.as_secs();
                let next_day = u32::try_from(days + 1)
                    .ok()
                    .and_then(|d| t.checked_add(DAY * d));
                (days as usize, next_day)
            }
            None => (0, None),
        };
        let mut headline = format!("No activity for {}.", count(days, "day"));
        if let Some(&n) = open_prs.get(&node.task.id).filter(|&&n| n > 0) {
            headline.push_str(&format!(" {}.", count(n, "open PR")));
        }
        let item = ItemFor {
            kind: NeedsKind::Quiet,
            key: format!("quiet:{}:{}", node.task.id.0, a.at),
            node,
            since: a.at.to_string(),
            headline,
            context: format!("Quiet for {}", count(days, "day")),
            target: Target::Task(node.task.id),
        };
        out.push(Entry {
            item: item.build(a.stale),
            tasks: vec![node.task.id],
            changes_at,
        });
    }
    // The next task to turn quiet. Turning quiet is all that can happen to one with no
    // new input, and the soonest is the one with the oldest activity.
    if let Some(t) = soonest
        .and_then(|s| from_ts(&act[s].at))
        .and_then(|t| t.checked_add(ctx.cfg.quiet_after))
    {
        wake.at(t);
    }
}
