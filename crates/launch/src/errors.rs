//! Every `ServiceError` launch returns, with its one sentence for the user.

use std::path::Path;
use std::time::Duration;

use airboss_core::TaskId;
use airboss_services::{ErrorCode, ServiceError};

use crate::brief::one_line;

/// The sentence `Availability::Missing` and `T3Missing` share, from the design.
pub const T3_MISSING: &str = "Do it and Plan it need T3 Code. Install it, then choose Check again.";

/// The sentence `Availability::Missing` and `HerdrMissing` share.
pub const HERDR_MISSING: &str = "Herdr isn't installed.";

pub(crate) fn t3_missing() -> ServiceError {
    ServiceError::new(ErrorCode::T3Missing, T3_MISSING)
}

pub(crate) fn t3_not_running() -> ServiceError {
    ServiceError::new(
        ErrorCode::T3NotRunning,
        "T3 Code isn't open. Open it, then try again.",
    )
}

pub(crate) fn t3_unreachable(detail: &str) -> ServiceError {
    ServiceError::new(
        ErrorCode::T3Unreachable,
        format!("Couldn't reach T3 Code: {}", short(detail)),
    )
}

pub(crate) fn t3_too_old() -> ServiceError {
    ServiceError::new(ErrorCode::T3TooOld, "Update T3 Code, then try again.")
}

pub(crate) fn timeout(after: Duration) -> ServiceError {
    ServiceError::new(
        ErrorCode::Timeout,
        format!(
            "T3 didn't answer in {} s. The thread may still appear; Airboss will link it if it does.",
            after.as_secs()
        ),
    )
}

pub(crate) fn not_a_project(place: &str) -> ServiceError {
    ServiceError::new(
        ErrorCode::NotAProject,
        format!("{} isn't a T3 project yet.", one_line(place)),
    )
}

pub(crate) fn repo_missing(place: &str, path: &Path) -> ServiceError {
    ServiceError::new(
        ErrorCode::RepoMissing,
        format!(
            "{} isn't at {} any more.",
            one_line(place),
            one_line(&path.display().to_string())
        ),
    )
}

pub(crate) fn not_a_full_path(place: &str, path: &Path) -> ServiceError {
    ServiceError::new(
        ErrorCode::RepoMissing,
        format!(
            "{}'s folder {} isn't a full path. Choose the folder again.",
            one_line(place),
            one_line(&path.display().to_string())
        ),
    )
}

pub(crate) fn no_base_branch(place: &str) -> ServiceError {
    ServiceError::new(
        ErrorCode::NoBaseBranch,
        format!(
            "Couldn't find {}'s main branch. Set one in the repo's settings.",
            one_line(place)
        ),
    )
}

pub(crate) fn bad_branch() -> ServiceError {
    ServiceError::new(
        ErrorCode::BadBranch,
        "Branch names can use letters, digits, `.`, `_`, `/` and `-`.",
    )
}

pub(crate) fn bad_answer() -> ServiceError {
    ServiceError::new(
        ErrorCode::BadAnswer,
        "T3 answered in a way Airboss doesn't understand.",
    )
}

pub(crate) fn t3_failed(line: &str) -> ServiceError {
    ServiceError::new(ErrorCode::T3Failed, short(line))
}

/// A pending row already exists for this place. `app` is where it's starting: "T3" or
/// "Herdr".
pub(crate) fn already_starting(repo: Option<&str>, app: &str) -> ServiceError {
    let what = match repo {
        Some(name) => one_line(name),
        None => "This task's thread".to_string(),
    };
    ServiceError::new(
        ErrorCode::AlreadyStarting,
        format!("{what} is still starting. Check {app}, or choose Start again."),
    )
}

/// Another call in this process is starting this place right now.
pub(crate) fn starting_now(repo: Option<&str>, app: &str) -> ServiceError {
    let what = match repo {
        Some(name) => one_line(name),
        None => "This task's thread".to_string(),
    };
    ServiceError::new(
        ErrorCode::AlreadyStarting,
        format!("{what} is starting right now. Wait a moment, then check {app}."),
    )
}

pub(crate) fn herdr_missing() -> ServiceError {
    ServiceError::new(ErrorCode::HerdrMissing, HERDR_MISSING)
}

pub(crate) fn herdr_not_running() -> ServiceError {
    ServiceError::new(
        ErrorCode::HerdrNotRunning,
        "Herdr didn't open. Open it and try again.",
    )
}

/// `herdr-app`'s own `{error}`, or what else it printed.
pub(crate) fn herdr_failed(message: &str) -> ServiceError {
    ServiceError::new(ErrorCode::HerdrFailed, short(message))
}

pub(crate) fn herdr_timeout(after: Duration) -> ServiceError {
    ServiceError::new(
        ErrorCode::Timeout,
        format!(
            "Herdr didn't answer in {} s. The tab may still appear; Airboss will link it if it does.",
            after.as_secs()
        ),
    )
}

/// A `herdr-app` call that makes nothing (listing spaces) didn't answer in time.
pub(crate) fn herdr_slow(after: Duration) -> ServiceError {
    ServiceError::new(
        ErrorCode::Timeout,
        format!(
            "Herdr didn't answer in {} s. Try again in a moment.",
            after.as_secs()
        ),
    )
}

pub(crate) fn herdr_bad_answer() -> ServiceError {
    ServiceError::new(
        ErrorCode::BadAnswer,
        "Herdr answered in a way Airboss doesn't understand.",
    )
}

/// git's own message, as one sentence: its first line, without `fatal: `, capitalised and
/// ending with a full stop.
pub(crate) fn worktree_failed(git_says: &str) -> ServiceError {
    let line = git_says
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("git couldn't make the worktree");
    let line = ["fatal:", "error:"]
        .iter()
        .find_map(|p| line.strip_prefix(p))
        .unwrap_or(line)
        .trim();
    let mut chars = line.chars();
    let mut sentence: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Git couldn't make the worktree".to_string(),
    };
    if !sentence.ends_with(['.', '!', '?']) {
        sentence.push('.');
    }
    ServiceError::new(ErrorCode::WorktreeFailed, short(&sentence))
}

pub(crate) fn task_proposed(task: TaskId) -> ServiceError {
    ServiceError::new(
        ErrorCode::TaskProposed,
        format!("Task {} is still a proposal. Accept it first.", task.0),
    )
}

pub(crate) fn task_closed(task: TaskId, status: airboss_core::Status) -> ServiceError {
    ServiceError::new(
        ErrorCode::TaskClosed,
        format!("Task {} is {status}. Reopen it to start it.", task.0),
    )
}

pub(crate) fn needs_choice(task: TaskId) -> ServiceError {
    ServiceError::new(
        ErrorCode::NeedsChoice,
        format!("Task {} has no repo. Choose where to start it.", task.0),
    )
}

/// "Task 146 waits on 143 and 145."
pub(crate) fn waiting(task: TaskId, on: &[TaskId]) -> ServiceError {
    let ids: Vec<String> = on.iter().map(|t| t.0.to_string()).collect();
    let list = match ids.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    ServiceError::new(
        ErrorCode::Waiting,
        format!("Task {} waits on {list}.", task.0),
    )
}

pub(crate) fn not_found(task: TaskId) -> ServiceError {
    ServiceError::new(
        ErrorCode::NotFound,
        format!("Task {} doesn't exist any more.", task.0),
    )
}

pub(crate) fn repo_not_found() -> ServiceError {
    ServiceError::new(ErrorCode::NotFound, "That repo isn't registered any more.")
}

pub(crate) fn core(e: &airboss_core::Error) -> ServiceError {
    ServiceError::new(
        ErrorCode::Core,
        format!("Airboss couldn't update the task: {e}."),
    )
}

pub(crate) fn storage(e: &rusqlite::Error) -> ServiceError {
    ServiceError::new(
        ErrorCode::Storage,
        format!("Airboss couldn't read or save its launch records: {e}."),
    )
}

pub(crate) fn no_database() -> ServiceError {
    ServiceError::new(
        ErrorCode::Storage,
        "Airboss has no database file to keep its launch records in.",
    )
}

pub(crate) fn git_failed(e: &std::io::Error) -> ServiceError {
    ServiceError::new(ErrorCode::Internal, format!("Couldn't run git: {e}."))
}

pub(crate) fn shutting_down() -> ServiceError {
    ServiceError::new(ErrorCode::ShuttingDown, "Airboss is closing.")
}

/// One line, at most 300 characters.
fn short(text: &str) -> String {
    let line = one_line(text.trim());
    match line.char_indices().nth(300) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_names_every_blocker() {
        let t = TaskId(146);
        assert_eq!(waiting(t, &[TaskId(143)]).message, "Task 146 waits on 143.");
        assert_eq!(
            waiting(t, &[TaskId(143), TaskId(145)]).message,
            "Task 146 waits on 143 and 145."
        );
        assert_eq!(
            waiting(t, &[TaskId(1), TaskId(2), TaskId(3)]).message,
            "Task 146 waits on 1, 2 and 3."
        );
    }

    #[test]
    fn gits_message_becomes_one_sentence() {
        let e = worktree_failed("fatal: 'task-1' is already used by worktree at '/x'\nmore");
        assert_eq!(e.code, ErrorCode::WorktreeFailed);
        assert_eq!(e.message, "'task-1' is already used by worktree at '/x'.");
        assert_eq!(
            worktree_failed("fatal: invalid reference: main").message,
            "Invalid reference: main."
        );
        assert_eq!(
            worktree_failed("").message,
            "Git couldn't make the worktree."
        );
    }

    #[test]
    fn long_t3_lines_are_cut() {
        let e = t3_failed(&format!("{}\nsecond", "x".repeat(400)));
        assert_eq!(e.message.chars().count(), 301);
    }
}
