//! Repos and links: `add_repos`, `pick_folder`, `add_link`, `remove_link`.

use std::path::{Path, PathBuf};

use airboss_core::{Actor, LinkId, LinkKind, PrState, Repo, RepoId, TaskId};
use airboss_services::PrRef;
use tauri_plugin_log::log;

use crate::classify::classify;
use crate::dto::{AddReposResult, Patch, RemoveRepoResult, path_text};
use crate::errors::{AppError, AppResult, Ctx, ErrorCode, MSG_REPO_EXISTS};
use crate::state::{AppState, Caller, lock};

/// The `origin` remote's URL from a repo's `.git/config`, if it has one.
pub fn origin_remote(folder: &Path) -> Option<String> {
    let text = std::fs::read_to_string(folder.join(".git/config")).ok()?;
    let mut in_origin = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line == "[remote \"origin\"]";
        } else if in_origin
            && let Some(url) = line
                .strip_prefix("url")
                .map(str::trim_start)
                .and_then(|l| l.strip_prefix('='))
        {
            return Some(url.trim().to_string());
        }
    }
    None
}

/// An absolute path to an existing folder, with symlinks resolved, so one folder is one repo.
fn folder(p: &str) -> AppResult<PathBuf> {
    let refuse = || {
        AppError::new(
            ErrorCode::InvalidTarget,
            "Choose a folder to add as a repo.",
        )
    };
    folder_or(p, refuse)
}

fn folder_or(p: &str, refuse: impl Fn() -> AppError) -> AppResult<PathBuf> {
    let path = Path::new(p);
    if !path.is_absolute() {
        return Err(refuse());
    }
    let path = path.canonicalize().map_err(|_| refuse())?;
    if !path.is_dir() {
        return Err(refuse());
    }
    Ok(path)
}

/// Adds each folder as a repo: its name from the folder, its remote from `.git/config`. A
/// folder that can't be added fails the call after the others are added (their patch is sent
/// as an event too).
pub fn add_repos(s: &AppState, _c: Caller, paths: Vec<String>) -> AppResult<AddReposResult> {
    let mut added: Vec<Repo> = Vec::new();
    let mut failed: Option<AppError> = None;
    for p in paths {
        let path = match folder(&p) {
            Ok(path) => path,
            Err(e) => {
                failed.get_or_insert(e.with_detail(p));
                continue;
            }
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&p)
            .to_string();
        let remote = origin_remote(&path);
        match s.core_as(Ctx::AddRepo, |st| {
            st.add_repo(&name, &path, remote.as_deref(), &Actor::Me)
        }) {
            Ok(r) => added.push(r),
            Err(e) => {
                failed.get_or_insert(e.with_detail(p));
            }
        }
    }
    // `add_repo` writes no event; catch-up compares `repos()` too.
    let patch = s.after_write()?;
    match failed {
        Some(e) => Err(e),
        None => Ok(AddReposResult {
            repos: added,
            patch,
        }),
    }
}

fn repo_gone() -> AppError {
    AppError::new(ErrorCode::NotFound, "That repo no longer exists.")
}

/// A refused repo change in the user's words.
fn repo_error(e: airboss_core::Error) -> AppError {
    match e {
        airboss_core::Error::NotFound => repo_gone(),
        airboss_core::Error::InvalidTarget => AppError::new(
            ErrorCode::InvalidTarget,
            "A repo needs a name and a folder.",
        ),
        airboss_core::Error::AlreadyExists => {
            AppError::new(ErrorCode::AlreadyExists, MSG_REPO_EXISTS)
        }
        e => AppError::core(e, Ctx::AddRepo),
    }
}

/// Settings › Repos: the repo as it should be. Only what changed is written: a new name, a
/// new folder (absolute, existing, symlinks resolved, as when adding), or a new remote; a
/// blank remote clears it. Tasks keep the repo, so they follow it.
pub fn edit_repo(
    s: &AppState,
    _c: Caller,
    repo: RepoId,
    name: String,
    path: String,
    remote: Option<String>,
) -> AppResult<Patch> {
    let current = s
        .vm(|vm| vm.repos.iter().find(|r| r.id == repo).cloned())
        .ok_or_else(repo_gone)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidTarget,
            "A repo needs a name.",
        ));
    }
    let path = if path == current.path {
        None
    } else {
        Some(folder_or(&path, || {
            AppError::new(ErrorCode::InvalidTarget, "Choose the repo's folder.")
        })?)
    };
    let remote = remote
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    let new_name = (name != current.name).then_some(name);
    let new_remote = (remote != current.remote_url).then_some(remote.as_deref());
    s.core(|st| st.edit_repo(repo, new_name, path.as_deref(), new_remote, &Actor::Me))
        .map_err(repo_error)?;
    // `edit_repo` writes no event; catch-up compares `repos()` too.
    s.after_write()
}

/// Settings › Repos › Remove (after the UI confirms with the count of tasks that lose the
/// link): unlinked from every task that has it, done ones too, then unregistered.
pub fn remove_repo(s: &AppState, _c: Caller, repo: RepoId) -> AppResult<RemoveRepoResult> {
    let unlinked = s
        .core(|st| st.remove_repo(repo, &Actor::Me))
        .map_err(repo_error)?;
    Ok(RemoveRepoResult {
        unlinked,
        patch: s.after_write()?,
    })
}

/// Settings › Database › "Back up…": a save dialog, then core's online backup, a consistent
/// copy while everything keeps running. `None` when the dialog is cancelled. The dialog has
/// already asked about replacing a file, so one there is overwritten.
pub fn back_up_database(s: &AppState, _c: Caller) -> AppResult<Option<String>> {
    let day = airboss_services::time::to_ts(s.now());
    let name = format!("Airboss backup {}.db", day.get(..10).unwrap_or(&day));
    let Some(to) = s.shell.pick_save(&name) else {
        return Ok(None);
    };
    // Not through `AppState::core`: a backup that can't be written is a failed backup, not
    // the database problem state.
    let copied = lock(s.store()).backup(&to, true);
    copied.map_err(|e| {
        let why = match e {
            airboss_core::Error::InvalidTarget => {
                "That's the database itself; choose another place.".to_string()
            }
            e => e.to_string(),
        };
        AppError::new(
            ErrorCode::Db,
            format!("Couldn't back up the database. {why}"),
        )
        .with_detail(to.display().to_string())
    })?;
    Ok(Some(path_text(&to)))
}

/// "Add another folder…": the folder picker; `None` when cancelled.
pub fn pick_folder(s: &AppState, _c: Caller) -> AppResult<Option<String>> {
    Ok(s.shell.pick_folder().map(|p| path_text(&p)))
}

/// Links a pasted link or path. A PR is then looked up in the background; if GitHub says it
/// already merged or closed, its state is corrected (as me). Its title shows from `sync`.
pub fn add_link(
    s: &AppState,
    _c: Caller,
    id: TaskId,
    text: String,
    title: Option<String>,
) -> AppResult<Patch> {
    let new = classify(&text, title.as_deref(), &s.config.home)?;
    let is_pr = new.kind == LinkKind::Pr;
    let link = s
        .core(|st| st.link(id, new, &Actor::Me))
        .map_err(|e| match e {
            airboss_core::Error::NotFound => AppError::task_gone(id),
            e => AppError::core(e, Ctx::Link),
        })?;
    let patch = s.after_write()?;
    if is_pr && let Some(pr) = PrRef::parse(&link.r#ref) {
        s.spawn("lookup-pr", move |s| lookup_pr(s, &pr));
    }
    Ok(patch)
}

/// Asks GitHub about a PR just linked; records it merged or closed if it already is.
fn lookup_pr(s: &AppState, pr: &PrRef) {
    let view = match s.services.sync.lookup_pr(pr) {
        Ok(v) => v,
        Err(e) => {
            log::info!("lookup of {pr} failed: {}", e.message);
            return;
        }
    };
    let state = match view.state {
        Some(PrState::Merged) => PrState::Merged,
        Some(PrState::Closed) => PrState::Closed,
        _ => return,
    };
    if let Err(e) = s.core(|st| st.set_pr_state(pr.as_str(), state, &Actor::Me)) {
        log::warn!("couldn't record {pr} as {state}: {e}");
    }
    if let Err(e) = s.catch_up() {
        log::warn!("catch-up after the PR lookup failed: {e}");
    }
}

/// Unlinks a link.
pub fn remove_link(s: &AppState, _c: Caller, link: LinkId) -> AppResult<Patch> {
    s.core_as(Ctx::Link, |st| st.unlink(link, &Actor::Me))?;
    s.after_write()
}
