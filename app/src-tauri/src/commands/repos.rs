//! Repos and links: `add_repos`, `pick_folder`, `add_link`, `remove_link`.

use std::path::{Path, PathBuf};

use airboss_core::{Actor, LinkId, LinkKind, PrState, Repo, TaskId};
use airboss_services::PrRef;
use tauri_plugin_log::log;

use crate::classify::classify;
use crate::dto::{AddReposResult, Patch, path_text};
use crate::errors::{AppError, AppResult, Ctx};
use crate::state::{AppState, Caller};

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

/// Adds each folder as a repo: its name from the folder, its remote from `.git/config`. A
/// folder that can't be added fails the call after the others are added (their patch is sent
/// as an event too).
pub fn add_repos(s: &AppState, _c: Caller, paths: Vec<String>) -> AppResult<AddReposResult> {
    let mut added: Vec<Repo> = Vec::new();
    let mut failed: Option<AppError> = None;
    for p in paths {
        let path = PathBuf::from(&p);
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
