//! The state rules for T3 threads: from what [`T3Read`] says to a [`ThreadView`].
//!
//! Pure functions; the T3 worker calls them after each read.

use std::collections::{HashMap, HashSet};

use airboss_services::time::from_ts;
use airboss_services::{
    Request, RequestKind, Source, ThreadKey, ThreadState, ThreadView, Timestamp,
};

use crate::t3::{RunStatus, T3Read, T3Request, T3RequestItem, T3RequestKind, T3Untracked};

/// What one read says about the linked threads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Linked {
    /// One view per linked thread id, in the order given; `gone` when T3 has no row.
    pub views: Vec<ThreadView>,
    /// Pending requests on these threads that have no turn item yet. They're retried on
    /// the next pass, and the worker marks T3 broken if one is still missing after 10 s.
    pub itemless: Vec<String>,
}

/// The phrase shown for an approval with no `prompt`.
pub fn approval_phrase(kind: T3RequestKind) -> &'static str {
    match kind {
        T3RequestKind::UserInput => "Answer a question",
        T3RequestKind::Command => "Run a command",
        T3RequestKind::FileRead => "Read a file",
        T3RequestKind::FileChange => "Change files",
        T3RequestKind::Permission => "Grant a permission",
        T3RequestKind::DynamicToolCall => "Use a tool",
        T3RequestKind::McpElicitation => "Answer an MCP server",
        T3RequestKind::AuthRefresh => "Sign in again",
    }
}

/// The note a failed run adds to its tasks: "Run failed: " and the first 200 characters.
pub fn failure_note(message: &str) -> String {
    let short: String = message.trim().chars().take(200).collect();
    format!("Run failed: {short}")
}

/// The text a failed run with no error item shows.
pub const RUN_FAILED: &str = "Run failed";

/// The newest of several times, compared as times.
fn latest<'a>(times: impl IntoIterator<Item = Option<&'a Timestamp>>) -> Option<Timestamp> {
    times
        .into_iter()
        .flatten()
        .filter_map(|t| from_ts(t).map(|at| (at, t)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, t)| t.clone())
}

fn t3_key(id: &str) -> ThreadKey {
    ThreadKey {
        source: Source::T3,
        r#ref: id.to_string(),
    }
}

/// A thread T3 has no row for.
pub fn gone_view(id: &str) -> ThreadView {
    ThreadView {
        key: t3_key(id),
        linked: true,
        state: ThreadState::Gone,
        title: None,
        request: None,
        error: None,
        run_id: None,
        project_path: None,
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: None,
        stale: false,
    }
}

/// The request a pending T3 request becomes, when its item is there.
fn request(q: &T3Request) -> Option<Request> {
    let (kind, text) = match (&q.item, q.kind) {
        (None, _) => return None,
        (Some(T3RequestItem::Question(text)), _) => (RequestKind::Question, text.clone()),
        (Some(T3RequestItem::Approval(prompt)), kind) => (
            RequestKind::Approval,
            prompt
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| approval_phrase(kind).to_string()),
        ),
    };
    Some(Request {
        id: q.id.clone(),
        kind,
        text,
        at: q.created_at.clone(),
    })
}

/// The views of the linked threads, first matching state rule wins (SPEC-sync, "State of a
/// linked thread"). `known_errors` holds the messages of failed runs read on earlier passes
/// (by run id), since Q4 only reads new ones.
pub fn linked(read: &T3Read, ids: &[String], known_errors: &HashMap<String, String>) -> Linked {
    let mut out = Linked::default();
    for id in ids {
        let Some(t) = read.threads.iter().find(|t| &t.id == id) else {
            out.views.push(gone_view(id));
            continue;
        };
        let mut view = gone_view(id);
        view.title = Some(t.title.clone());
        view.project_path = t.project_path.clone();
        view.branch = t.branch.clone();
        view.worktree_path = t.worktree_path.clone();
        view.parent_ref = t.parent.clone();
        view.child_count = read.children.get(id).copied().unwrap_or(0);
        if t.gone {
            out.views.push(view);
            continue;
        }
        let run = read.runs.iter().find(|r| &r.thread_id == id);
        let mine: Vec<&T3Request> = read
            .requests
            .iter()
            .filter(|q| &q.thread_id == id || q.root.as_ref() == Some(id))
            .collect();
        out.itemless.extend(
            mine.iter()
                .filter(|q| q.item.is_none())
                .map(|q| q.id.clone()),
        );
        let pending = mine.iter().find_map(|q| request(q));
        view.run_id = run.map(|r| r.run_id.clone());
        view.last_activity_at = latest([
            run.map(|r| &r.requested_at),
            run.and_then(|r| r.started_at.as_ref()),
            run.and_then(|r| r.completed_at.as_ref()),
            pending.as_ref().map(|p| &p.at),
        ])
        .or_else(|| Some(t.updated_at.clone()));
        let status = run.map(|r| r.status);
        view.state = if let Some(p) = pending {
            view.request = Some(p);
            ThreadState::NeedsMe
        } else if status.is_some_and(RunStatus::is_active) {
            ThreadState::Running
        } else if t.settled_at.is_some() {
            ThreadState::Settled
        } else if status == Some(RunStatus::Failed) {
            let run_id = run.map(|r| r.run_id.as_str()).unwrap_or_default();
            view.error = Some(
                read.failures
                    .get(run_id)
                    .cloned()
                    .flatten()
                    .or_else(|| known_errors.get(run_id).cloned())
                    .unwrap_or_else(|| RUN_FAILED.to_string()),
            );
            ThreadState::Errored
        } else {
            ThreadState::Idle
        };
        out.views.push(view);
    }
    out
}

/// Q6 rows linked to no task, as `settled` or `idle` (their runs aren't read).
pub fn untracked(rows: &[T3Untracked], linked_anywhere: &HashSet<String>) -> Vec<ThreadView> {
    rows.iter()
        .filter(|t| !linked_anywhere.contains(&t.id))
        .map(|t| {
            let mut view = gone_view(&t.id);
            view.linked = false;
            view.state = if t.settled {
                ThreadState::Settled
            } else {
                ThreadState::Idle
            };
            view.title = Some(t.title.clone());
            view.branch = t.branch.clone();
            view.project_path = t.project_path.clone();
            view.last_activity_at = Some(t.updated_at.clone());
            view
        })
        .collect()
}

/// Everything but `A–Z a–z 0–9 - _`, percent-encoded.
fn encode(part: &str) -> String {
    let mut out = String::new();
    for b in part.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `t3code://threads/<env>/<thread>` for a T3 key; `None` for a Claude session or when the
/// environment id isn't known.
pub fn thread_url(environment: Option<&str>, key: &ThreadKey) -> Option<String> {
    match (key.source, environment) {
        (Source::T3, Some(env)) => Some(format!(
            "t3code://threads/{}/{}",
            encode(env),
            encode(&key.r#ref)
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_encode_everything_but_the_safe_set() {
        let key = |r: &str| ThreadKey {
            source: Source::T3,
            r#ref: r.into(),
        };
        assert_eq!(
            thread_url(Some("0b8a-1f"), &key("mcp:launch:abc")).as_deref(),
            Some("t3code://threads/0b8a-1f/mcp%3Alaunch%3Aabc")
        );
        assert_eq!(
            thread_url(Some("e"), &key("a/b c_d")).as_deref(),
            Some("t3code://threads/e/a%2Fb%20c_d")
        );
        assert_eq!(thread_url(None, &key("a")), None);
        let claude = ThreadKey {
            source: Source::Claude,
            r#ref: "s".into(),
        };
        assert_eq!(thread_url(Some("e"), &claude), None);
    }

    #[test]
    fn notes_are_cut_at_200_characters() {
        let long = "é".repeat(300);
        let note = failure_note(&long);
        assert_eq!(note.chars().count(), "Run failed: ".len() + 200);
        assert_eq!(failure_note("  boom \n"), "Run failed: boom");
    }
}
