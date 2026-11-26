//! Do it and Plan it: the in-flight guard, the "where to start" choice, and the toasts for a
//! launch from quick add. `launch.start` runs `t3` and may take seconds, so it's always called
//! with no lock held.

use airboss_core::{Actor, RepoId, Status, TaskId};
use airboss_services::{
    ErrorCode as SvcCode, LaunchReport, Launcher, Mode, Place, Prepared, StartRequest, ThreadKey,
};

use crate::dto::{
    DoItPlace, DoItResult, ToastArgs, ToastEvent, ToastEventAction, ToastTone, path_text,
};
use crate::errors::{AppError, AppResult, Ctx, ErrorCode};
use crate::state::{AppState, Caller};

/// Do it: one thread per linked repo on one branch, or the Start in menu with no repo.
pub fn do_it(
    s: &AppState,
    c: Caller,
    id: TaskId,
    place: Option<DoItPlace>,
    again: Option<bool>,
) -> AppResult<DoItResult> {
    let again = again.unwrap_or(false);
    let _guard = s.begin_launch(id)?;
    let place = match place.unwrap_or(DoItPlace::Linked) {
        DoItPlace::Repo(repo) => {
            s.core_as(Ctx::Task(id), |st| st.link_repo(id, repo, &Actor::Me))?;
            s.after_write()?;
            Place::TaskRepos
        }
        DoItPlace::NoRepo => {
            let prepared = prepare(s, c, id, Mode::DoIt)?;
            Place::NoRepo(prepared.research_dir)
        }
        DoItPlace::Linked => {
            let prepared = prepare(s, c, id, Mode::DoIt)?;
            if prepared.needs_choice {
                return Ok(DoItResult::NeedsChoice {
                    choices: prepared.choices,
                    research_dir: path_text(&prepared.research_dir),
                });
            }
            Place::TaskRepos
        }
    };
    start(s, c, id, Mode::DoIt, place, again)
}

/// Plan it: one planning thread in the task's repos.
pub fn plan_it(s: &AppState, c: Caller, id: TaskId, again: Option<bool>) -> AppResult<DoItResult> {
    let _guard = s.begin_launch(id)?;
    start(
        s,
        c,
        id,
        Mode::PlanIt,
        Place::TaskRepos,
        again.unwrap_or(false),
    )
}

/// "Reopen and start": the task back to todo, then Do it.
pub fn reopen_and_start(s: &AppState, c: Caller, id: TaskId) -> AppResult<DoItResult> {
    s.core_as(Ctx::Task(id), |st| {
        st.set_status(id, Status::Todo, None, &Actor::Me)
    })?;
    s.after_write()?;
    do_it(s, c, id, None, Some(false))
}

/// "Add to T3": makes the repo a T3 project, so a retry can start there.
pub fn add_to_t3(s: &AppState, _c: Caller, repo: RepoId) -> AppResult<()> {
    s.services.launch.add_to_t3(repo).map_err(AppError::from)
}

fn prepare(s: &AppState, c: Caller, id: TaskId, mode: Mode) -> AppResult<Prepared> {
    s.services.launch.prepare(id, mode).map_err(|e| {
        let e = AppError::from(e);
        s.surface(c, id, error_toast(id, &e, mode));
        e
    })
}

fn start(
    s: &AppState,
    c: Caller,
    id: TaskId,
    mode: Mode,
    place: Place,
    again: bool,
) -> AppResult<DoItResult> {
    let req = StartRequest {
        task: id,
        mode,
        place,
        again,
        // TODO(wave9/app): the launcher the "Start agents in" setting picks.
        launcher: Launcher::T3,
    };
    let report = match s.services.launch.start(&req) {
        Ok(r) => r,
        Err(e) => {
            let e = AppError::from(e);
            s.surface(c, id, error_toast(id, &e, mode));
            if matches!(e.code, ErrorCode::T3Missing | ErrorCode::T3NotRunning) {
                s.refresh_connections();
            }
            return Err(e);
        }
    };
    // `launch` wrote to `core` (the move to doing, the thread links): catch up.
    let patch = s.after_write()?;
    if !report.failed.is_empty() {
        let names = s.vm(|vm| {
            vm.repos
                .iter()
                .map(|r| (r.id, r.name.clone()))
                .collect::<Vec<_>>()
        });
        let name = |r: Option<RepoId>| {
            r.and_then(|r| {
                names
                    .iter()
                    .find(|(id, _)| *id == r)
                    .map(|(_, n)| n.clone())
            })
            .unwrap_or_else(|| "the research folder".into())
        };
        s.surface(c, id, report_toast(id, mode, &report, &name));
    }
    if report
        .failed
        .iter()
        .any(|(_, e)| matches!(e.code, SvcCode::T3Missing | SvcCode::T3NotRunning))
    {
        s.refresh_connections();
    }
    Ok(DoItResult::Started { report, patch })
}

fn action(label: &str, command: &str, args: ToastArgs) -> ToastEventAction {
    ToastEventAction {
        label: label.into(),
        command: command.into(),
        args,
    }
}

/// The same request again (`launch` skips what already started), or with `again`.
fn retry(id: TaskId, mode: Mode, again: bool, label: &str) -> ToastEventAction {
    match mode {
        Mode::DoIt => action(
            label,
            "do_it",
            ToastArgs::DoIt {
                id,
                place: None,
                again,
            },
        ),
        Mode::PlanIt => action(label, "plan_it", ToastArgs::PlanIt { id, again }),
    }
}

fn open_thread(key: &ThreadKey) -> ToastEventAction {
    action(
        "Open thread",
        "open_thread",
        ToastArgs::Thread { key: key.clone() },
    )
}

/// The toast for a call that failed outright: Retry, or Reopen and start for a closed task.
fn error_toast(id: TaskId, e: &AppError, mode: Mode) -> ToastEvent {
    let actions = match e.code {
        ErrorCode::TaskClosed => vec![action(
            "Reopen and start",
            "reopen_and_start",
            ToastArgs::Task { id },
        )],
        ErrorCode::NeedsChoice | ErrorCode::AlreadyStarting => Vec::new(),
        _ => vec![retry(id, mode, false, "Retry")],
    };
    ToastEvent {
        text: format!("Couldn't start #{}: {}", id.0, e.message),
        actions,
        undo: None,
        tone: Some(ToastTone::Warning),
        task: Some(id),
    }
}

/// The spec's result table ("Do it and Plan it results").
pub fn report_toast(
    id: TaskId,
    mode: Mode,
    r: &LaunchReport,
    repo_name: &dyn Fn(Option<RepoId>) -> String,
) -> ToastEvent {
    let task = id.0;
    let fixes = |repo: Option<RepoId>, code: SvcCode| match (repo, code) {
        (Some(repo), SvcCode::NotAProject) => vec![
            action("Add to T3", "add_to_t3", ToastArgs::Repo { repo }),
            retry(id, mode, false, "Retry"),
        ],
        _ => vec![retry(id, mode, false, "Retry")],
    };
    let (text, tone, actions) = match (r.started.as_slice(), r.failed.as_slice()) {
        ([], []) if !r.skipped.is_empty() => {
            let names: Vec<String> = r.skipped.iter().map(|r| repo_name(Some(*r))).collect();
            (
                format!("#{task} already has threads in {}", and_list(&names)),
                ToastTone::Info,
                vec![retry(id, mode, true, "Start again")],
            )
        }
        ([], [(repo, e), ..]) => (
            format!("Couldn't start #{task}: {}", e.message),
            ToastTone::Warning,
            fixes(*repo, e.code),
        ),
        ([], []) => (
            format!("Couldn't start #{task}: nothing to start."),
            ToastTone::Warning,
            vec![retry(id, mode, false, "Retry")],
        ),
        (started, [(repo, e), ..]) => {
            let ok: Vec<String> = started.iter().map(|t| repo_name(t.repo)).collect();
            (
                format!(
                    "Started in {}. {} failed: {}",
                    and_list(&ok),
                    repo_name(*repo),
                    e.message
                ),
                ToastTone::Warning,
                fixes(*repo, e.code),
            )
        }
        ([one], []) if mode == Mode::PlanIt => (
            format!(
                "Started a planning thread for #{task}. Its steps become subtasks when you settle the plan."
            ),
            ToastTone::Info,
            vec![open_thread(&one.key)],
        ),
        ([one], []) if one.repo.is_none() => (
            format!("Started a research thread for #{task}"),
            ToastTone::Info,
            vec![open_thread(&one.key)],
        ),
        ([one], []) => (
            format!("Started a thread for #{task} in {}", repo_name(one.repo)),
            ToastTone::Info,
            vec![open_thread(&one.key)],
        ),
        (many, []) => (
            match &r.branch {
                Some(b) => format!("Started {} threads for #{task} on {b}", many.len()),
                None => format!("Started {} threads for #{task}", many.len()),
            },
            ToastTone::Info,
            Vec::new(),
        ),
    };
    ToastEvent {
        text,
        actions,
        undo: None,
        tone: Some(tone),
        task: Some(id),
    }
}

/// "a", "a and b", "a, b and c".
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_read_naturally() {
        assert_eq!(and_list(&["a".into()]), "a");
        assert_eq!(and_list(&["a".into(), "b".into()]), "a and b");
        assert_eq!(
            and_list(&["a".into(), "b".into(), "c".into()]),
            "a, b and c"
        );
    }
}
