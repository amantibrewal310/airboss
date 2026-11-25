//! Every `ServiceError` launch returns, with its one sentence for the user.

use std::path::Path;
use std::time::Duration;

use airboss_core::TaskId;
use airboss_services::{ErrorCode, ServiceError};

use crate::brief::one_line;

/// The sentence `Availability::Missing` and `T3Missing` share, from the design.
pub const T3_MISSING: &str = "Do it and Plan it need T3 Code. Install it, then choose Check again.";

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

/// A pending row already exists for this place.
pub(crate) fn already_starting(repo: Option<&str>) -> ServiceError {
    let what = match repo {
        Some(name) => one_line(name),
        None => "This task's thread".to_string(),
    };
    ServiceError::new(
        ErrorCode::AlreadyStarting,
        format!("{what} is still starting. Check T3, or choose Start again."),
    )
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
    fn long_t3_lines_are_cut() {
        let e = t3_failed(&format!("{}\nsecond", "x".repeat(400)));
        assert_eq!(e.message.chars().count(), 301);
    }
}
