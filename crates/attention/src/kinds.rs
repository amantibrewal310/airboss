//! One function per kind (Quiet is in `quiet.rs`). Each adds its entries, unsorted and
//! unhidden; `compute` orders and hides them.

use airboss_core::TaskNode;
use airboss_services::{NeedsKind, RequestKind, Target, ThreadState, ThreadView};

use crate::compute::{Ctx, Entry, ItemFor, stamp};

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

/// Reviews. Added in the next step.
pub(crate) fn reviews(_ctx: &Ctx<'_>, _out: &mut Vec<Entry>) {}

/// Proposed subtasks. Added in a later step.
pub(crate) fn proposed(_ctx: &Ctx<'_>, _out: &mut Vec<Entry>) {}

/// Tasks that became ready. Added in a later step.
pub(crate) fn ready(_ctx: &Ctx<'_>, _out: &mut Vec<Entry>) {}

/// Parents whose subtasks are all done. Added in a later step.
pub(crate) fn finish(_ctx: &Ctx<'_>, _out: &mut Vec<Entry>) {}
