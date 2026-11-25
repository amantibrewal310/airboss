//! `AppError`, what every command returns on failure, and the message table (Appendix D).

use std::fmt;

use airboss_core::TaskId;
use airboss_services::ServiceError;
use serde::{Deserialize, Serialize};
use tauri_plugin_log::log;
use ts_rs::TS;

/// What kind of failure an [`AppError`] is. The UI branches on a few (`needs_choice`,
/// `not_a_project`, `task_closed`, `t3_missing`, `t3_not_running`, `already_starting`,
/// `db_unavailable`); every other code only shows its message.
///
/// The service codes mirror `airboss_services::ErrorCode` one for one, so a service's error
/// passes through with its own code and message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "AppErrorCode", export_to = "types.ts")]
pub enum ErrorCode {
    // The app's and core's own.
    EmptyTitle,
    NotFound,
    InvalidTarget,
    Loop,
    AlreadyExists,
    NotAllowed,
    InvalidTransition,
    Db,
    AlreadyStarting,
    UndoRefused,
    NotALink,
    UrlNotAllowed,
    DbUnavailable,
    HotkeyInUse,
    HotkeyReserved,
    Internal,
    // Passed through from a service.
    T3Missing,
    T3NotRunning,
    T3Unreachable,
    T3TooOld,
    Timeout,
    NotAProject,
    RepoMissing,
    NoBaseBranch,
    BadBranch,
    TaskClosed,
    TaskProposed,
    NeedsChoice,
    Waiting,
    BadAnswer,
    T3Failed,
    GhMissing,
    SignedOut,
    NoAccess,
    PrNotFound,
    BadPrRef,
    RateLimited,
    GhFailed,
    T3Unavailable,
    Core,
    Storage,
    ShuttingDown,
}

impl From<airboss_services::ErrorCode> for ErrorCode {
    fn from(code: airboss_services::ErrorCode) -> ErrorCode {
        use airboss_services::ErrorCode as S;
        match code {
            S::T3Missing => ErrorCode::T3Missing,
            S::T3NotRunning => ErrorCode::T3NotRunning,
            S::T3Unreachable => ErrorCode::T3Unreachable,
            S::T3TooOld => ErrorCode::T3TooOld,
            S::Timeout => ErrorCode::Timeout,
            S::NotAProject => ErrorCode::NotAProject,
            S::RepoMissing => ErrorCode::RepoMissing,
            S::NoBaseBranch => ErrorCode::NoBaseBranch,
            S::BadBranch => ErrorCode::BadBranch,
            S::TaskClosed => ErrorCode::TaskClosed,
            S::TaskProposed => ErrorCode::TaskProposed,
            S::NeedsChoice => ErrorCode::NeedsChoice,
            S::Waiting => ErrorCode::Waiting,
            S::AlreadyStarting => ErrorCode::AlreadyStarting,
            S::BadAnswer => ErrorCode::BadAnswer,
            S::T3Failed => ErrorCode::T3Failed,
            S::GhMissing => ErrorCode::GhMissing,
            S::SignedOut => ErrorCode::SignedOut,
            S::NoAccess => ErrorCode::NoAccess,
            S::PrNotFound => ErrorCode::PrNotFound,
            S::BadPrRef => ErrorCode::BadPrRef,
            S::RateLimited => ErrorCode::RateLimited,
            S::GhFailed => ErrorCode::GhFailed,
            S::T3Unavailable => ErrorCode::T3Unavailable,
            S::NotFound => ErrorCode::NotFound,
            S::Core => ErrorCode::Core,
            S::Storage => ErrorCode::Storage,
            S::Internal => ErrorCode::Internal,
            S::ShuttingDown => ErrorCode::ShuttingDown,
        }
    }
}

/// Why a command failed: a code the UI may branch on, and one sentence to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "types.ts")]
pub struct AppError {
    pub code: ErrorCode,
    /// One sentence for the user.
    pub message: String,
    /// More for the log or a full-window state, e.g. the database's path and SQLite's reason.
    pub detail: Option<String>,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

/// What a failed `core` call was doing, so its error can say it in the user's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    /// A change to one task.
    Task(TaskId),
    /// Moving a task, or adding one under a parent.
    Move(TaskId),
    /// `id` waiting on `waits_on`.
    Dependency { id: TaskId, waits_on: TaskId },
    /// Setting a task's branch.
    Branch(TaskId),
    /// Linking or unlinking a link.
    Link,
    /// Linking or unlinking a repo on a task.
    Repo(TaskId),
    /// Adding a repo.
    AddRepo,
    /// A read with no particular task.
    Read,
}

pub const MSG_EMPTY_TITLE: &str = "A task needs a title.";
pub const MSG_LINK_GONE: &str = "That link no longer exists.";
pub const MSG_UNDER_CLOSED: &str = "Tasks can't go under a proposed, done or dropped task.";
pub const MSG_DEP_FAMILY: &str = "A task can't wait on its own parent or subtask.";
pub const MSG_BRANCH: &str = "Branch names can use letters, digits, `.`, `_`, `/` and `-`.";
pub const MSG_REPO_EXISTS: &str = "A repo with that name or folder is already added.";
pub const MSG_NOT_ALLOWED: &str = "That change isn't allowed.";
pub const MSG_DB: &str =
    "Couldn't save. The database is busy or unreadable; details are in the log.";
pub const MSG_ALREADY_STARTING: &str = "Already starting — check T3 in a moment.";
pub const MSG_INTERNAL: &str = "Something went wrong inside Airboss; details are in the log.";
pub const MSG_NOT_A_LINK: &str = "Paste a link or a file path.";
pub const MSG_BAD_PR: &str = "That isn't a GitHub pull request.";
pub const MSG_URL_NOT_ALLOWED: &str = "Airboss only opens https links, files and T3 threads.";
pub const MSG_FILE_GONE: &str = "That file no longer exists.";
pub const MSG_FILE_UNREADABLE: &str = "Airboss can't reach that file.";
pub const MSG_HOTKEY_IN_USE: &str = "That key is in use by another app. Pick another.";
pub const MSG_NEEDS_REASON: &str = "Say why it's blocked.";
pub const MSG_MOVE_LOOP: &str = "A task can't go under itself or one of its subtasks.";

impl AppError {
    /// An error with a code and its sentence.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> AppError {
        AppError {
            code,
            message: message.into(),
            detail: None,
        }
    }

    /// Adds detail for the log or a full-window state.
    pub fn with_detail(mut self, detail: impl Into<String>) -> AppError {
        self.detail = Some(detail.into());
        self
    }

    /// "Something went wrong inside Airboss", with what happened in `detail` (logged).
    pub fn internal(detail: impl Into<String>) -> AppError {
        let detail = detail.into();
        log::error!("internal error: {detail}");
        AppError::new(ErrorCode::Internal, MSG_INTERNAL).with_detail(detail)
    }

    /// "Task 144 no longer exists."
    pub fn task_gone(id: TaskId) -> AppError {
        AppError::new(
            ErrorCode::NotFound,
            format!("Task {} no longer exists.", id.0),
        )
    }

    /// "144 changed since; nothing was undone."
    pub fn undo_refused(id: TaskId) -> AppError {
        AppError::new(
            ErrorCode::UndoRefused,
            format!("{} changed since; nothing was undone.", id.0),
        )
    }

    /// The in-flight guard's answer to a second Do it for the same task.
    pub fn already_starting() -> AppError {
        AppError::new(ErrorCode::AlreadyStarting, MSG_ALREADY_STARTING)
    }

    /// A `core` error, in the words of what the call was doing (Appendix D).
    pub fn core(err: airboss_core::Error, ctx: Ctx) -> AppError {
        use airboss_core::Error as E;
        match err {
            E::EmptyTitle => AppError::new(ErrorCode::EmptyTitle, MSG_EMPTY_TITLE),
            E::NotFound => match ctx {
                Ctx::Task(id) | Ctx::Move(id) | Ctx::Branch(id) => AppError::task_gone(id),
                Ctx::Dependency { id, .. } => AppError::task_gone(id),
                Ctx::Repo(id) => AppError::new(
                    ErrorCode::NotFound,
                    format!("That repo isn't linked to {}.", id.0),
                ),
                Ctx::Link => AppError::new(ErrorCode::NotFound, MSG_LINK_GONE),
                Ctx::AddRepo | Ctx::Read => {
                    AppError::new(ErrorCode::NotFound, "That no longer exists.")
                }
            },
            E::InvalidTarget => match ctx {
                Ctx::Move(_) => AppError::new(ErrorCode::InvalidTarget, MSG_UNDER_CLOSED),
                Ctx::Dependency { .. } => AppError::new(ErrorCode::InvalidTarget, MSG_DEP_FAMILY),
                Ctx::Branch(_) => AppError::new(ErrorCode::InvalidTarget, MSG_BRANCH),
                Ctx::Link => AppError::new(ErrorCode::InvalidTarget, MSG_BAD_PR),
                _ => not_allowed("InvalidTarget"),
            },
            E::Loop => match ctx {
                Ctx::Dependency { id, waits_on } => AppError::new(
                    ErrorCode::Loop,
                    format!(
                        "That would make a loop: {} already waits on {}.",
                        waits_on.0, id.0
                    ),
                ),
                _ => AppError::new(ErrorCode::Loop, MSG_MOVE_LOOP),
            },
            E::AlreadyExists => AppError::new(ErrorCode::AlreadyExists, MSG_REPO_EXISTS),
            E::NotAllowed => not_allowed("NotAllowed"),
            E::InvalidTransition => {
                log::warn!("core refused a change as me: InvalidTransition ({ctx:?})");
                AppError::new(ErrorCode::InvalidTransition, MSG_NOT_ALLOWED)
                    .with_detail("InvalidTransition")
            }
            E::NoteRequired => AppError::new(ErrorCode::NotAllowed, MSG_NEEDS_REASON),
            E::DuplicateKey(key) => not_allowed(&format!("DuplicateKey({key})")),
            E::Db(e) => {
                log::error!("database error ({ctx:?}): {e}");
                AppError::new(ErrorCode::Db, MSG_DB).with_detail(e.to_string())
            }
        }
    }
}

/// "That change isn't allowed." It shouldn't happen as me, so it's logged with its detail.
fn not_allowed(detail: &str) -> AppError {
    log::warn!("core refused a change as me: {detail}");
    AppError::new(ErrorCode::NotAllowed, MSG_NOT_ALLOWED).with_detail(detail)
}

impl From<ServiceError> for AppError {
    /// A service's error passes through with its own code and message.
    fn from(e: ServiceError) -> AppError {
        AppError::new(e.code.into(), e.message)
    }
}

/// Shorthand for command results.
pub type AppResult<T> = Result<T, AppError>;
