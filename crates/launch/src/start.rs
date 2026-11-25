//! `start`: one thread per place, each behind a pending row committed before `t3` runs.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use airboss_core::{LinkKind, NewLink, Repo, RepoId, TaskId};
use airboss_services::{
    ErrorCode, LaunchReport, Mode, Place, ServiceError, StartRequest, StartedThread,
};

use crate::brief::{self, RepoPlace};
use crate::db::{Inserted, NewRow, State};
use crate::prepare::{Facts, core_error, started_thread};
use crate::t3cli::{self, Answer};
use crate::{Inner, LaunchMsg, PlaceKey, branch, errors, lock, log, settle};

/// The most `-N` suffixes tried for an again-branch.
const AGAIN_MAX: u32 = 999;

/// Exactly what `start` would send to one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    /// `None` for a folder with no repo.
    pub repo: Option<RepoId>,
    pub argv: Vec<String>,
    pub stdin: String,
}

/// Where one thread starts.
struct Spot {
    repo: Option<Repo>,
    folder: PathBuf,
}

impl Spot {
    /// How messages name it: the repo, or the folder.
    fn label(&self) -> String {
        match &self.repo {
            Some(r) => r.name.clone(),
            None => self.folder.display().to_string(),
        }
    }
}

/// What a whole request comes to before any place is tried.
struct Request {
    facts: Facts,
    spots: Vec<Spot>,
    /// Do it in repos: the task's branch, checked.
    branch: Option<String>,
}

/// What one place comes to after its checks.
enum Planned {
    /// A thread already started here.
    Skip,
    Fail(ServiceError),
    Go(Go),
}

struct Go {
    /// The newest row id of this place when it was checked; "Start again" replaces only
    /// rows up to it.
    seen: i64,
    argv: Vec<String>,
    stdin: String,
    title: String,
    /// The branch and its base, for Do it in a repo.
    branch: Option<(String, String)>,
}

/// Step 1 of `start`: the checks, the places, and the branch (checked, not saved).
fn request(inner: &Inner, req: &StartRequest) -> Result<Request, ServiceError> {
    let facts = Facts::load(inner, req.task)?;
    facts.check_open()?;
    if req.mode == Mode::DoIt && !facts.waits_on.is_empty() {
        let on: Vec<TaskId> = facts.waits_on.iter().map(|t| t.id).collect();
        return Err(errors::waiting(req.task, &on));
    }
    let (repos, _) = facts.task_repos();
    let in_repo = |r: Repo| Spot {
        folder: PathBuf::from(&r.path),
        repo: Some(r),
    };
    let folder = |dir: &Path| Spot {
        repo: None,
        folder: dir.to_path_buf(),
    };
    let spots = match (&req.place, req.mode) {
        (Place::NoRepo(dir), _) => vec![folder(dir)],
        (Place::TaskRepos, Mode::DoIt) if repos.is_empty() => {
            return Err(errors::needs_choice(req.task));
        }
        (Place::TaskRepos, Mode::DoIt) => repos.into_iter().map(in_repo).collect(),
        (Place::TaskRepos, Mode::PlanIt) => match repos.into_iter().next() {
            Some(first) => vec![in_repo(first)],
            None => vec![folder(&inner.cfg.research_dir)],
        },
    };
    let branch = if req.mode == Mode::DoIt && spots.iter().any(|s| s.repo.is_some()) {
        let name = facts.branch();
        if !valid_branch(inner, &name)? {
            return Err(errors::bad_branch());
        }
        Some(name)
    } else {
        None
    };
    Ok(Request {
        facts,
        spots,
        branch,
    })
}

/// `core`'s rule, then `git check-ref-format --branch`.
fn valid_branch(inner: &Inner, name: &str) -> Result<bool, ServiceError> {
    if !branch::follows_rule(name) {
        return Ok(false);
    }
    lock(&inner.git)
        .git
        .valid_branch_name(name)
        .map_err(|e| errors::git_failed(&e))
}

/// The repo's base: the override, else its default branch. It must follow `core`'s rule,
/// since it goes to `t3` and into the brief.
fn base_of(inner: &Inner, repo: &Repo, path: &Path) -> Result<String, ServiceError> {
    let base_override = lock(&inner.db).base_ref(repo.id)?;
    let base = match base_override {
        Some(b) => Some(b),
        None => lock(&inner.git)
            .default_branch(path)
            .map_err(|e| errors::git_failed(&e))?,
    };
    match base {
        Some(b) if branch::follows_rule(&b) => Ok(b),
        _ => Err(errors::no_base_branch(&repo.name)),
    }
}

/// "Start again" in a repo that may hold a thread on `branch`: the first `<branch>-N`, from
/// 2, that no row of this task used and the repo doesn't have. Its base is `branch` if the
/// repo has it, else `base`.
fn again_branch(
    inner: &Inner,
    task: TaskId,
    path: &Path,
    branch: &str,
    base: &str,
) -> Result<(String, String), ServiceError> {
    let git_err = |e: std::io::Error| errors::git_failed(&e);
    for n in 2..=AGAIN_MAX {
        let name = format!("{branch}-{n}");
        if !valid_branch(inner, &name)? {
            return Err(errors::bad_branch());
        }
        if lock(&inner.db).branch_used(task, &name)? {
            continue;
        }
        if lock(&inner.git)
            .git
            .has_branch(path, &name)
            .map_err(git_err)?
        {
            continue;
        }
        let from = if lock(&inner.git)
            .git
            .has_branch(path, branch)
            .map_err(git_err)?
        {
            branch.to_string()
        } else {
            base.to_string()
        };
        return Ok((name, from));
    }
    Err(errors::bad_branch())
}

/// Checks one place and works out exactly what to send. Writes nothing.
fn plan(
    inner: &Inner,
    r: &Request,
    spot: &Spot,
    req: &StartRequest,
) -> Result<Planned, ServiceError> {
    let task = &r.facts.task;
    let rows: Vec<_> = lock(&inner.db)
        .rows_for_task(task.id)?
        .into_iter()
        .filter(|row| row.mode == req.mode && row.repo == spot.repo.as_ref().map(|x| x.id))
        .collect();
    if !req.again {
        if rows.iter().any(|row| row.state == State::Started) {
            return Ok(Planned::Skip);
        }
        if rows.iter().any(|row| row.state == State::Pending) {
            let name = spot.repo.as_ref().map(|x| x.name.as_str());
            return Ok(Planned::Fail(errors::already_starting(name)));
        }
    }
    let seen = rows.iter().map(|row| row.id).max().unwrap_or(0);
    let label = spot.label();
    if !spot.folder.is_absolute() {
        return Ok(Planned::Fail(errors::not_a_full_path(&label, &spot.folder)));
    }
    if !spot.folder.is_dir() {
        return Ok(Planned::Fail(errors::repo_missing(&label, &spot.folder)));
    }
    let folder = spot.folder.display().to_string();
    let reference = r.facts.reference();
    let nonce = brief::nonce();
    let several = r.spots.len() > 1;
    let go = match (req.mode, &spot.repo, &r.branch) {
        (Mode::DoIt, Some(repo), Some(task_branch)) => {
            let base = match base_of(inner, repo, &spot.folder) {
                Ok(b) => b,
                Err(e) => return Ok(Planned::Fail(e)),
            };
            let may_hold_branch = rows
                .iter()
                .any(|row| matches!(row.state, State::Started | State::Pending | State::Lost));
            let (branch, base) = if req.again && may_hold_branch {
                match again_branch(inner, task.id, &spot.folder, task_branch, &base) {
                    Ok(pair) => pair,
                    Err(e) => return Ok(Planned::Fail(e)),
                }
            } else {
                (task_branch.clone(), base)
            };
            let all: Vec<RepoPlace> = r
                .spots
                .iter()
                .filter_map(|s| s.repo.as_ref())
                .map(|x| RepoPlace {
                    name: x.name.clone(),
                    path: x.path.clone(),
                })
                .collect();
            let here = RepoPlace {
                name: repo.name.clone(),
                path: folder.clone(),
            };
            let title =
                t3cli::thread_title(task.id, &task.title, several.then_some(repo.name.as_str()));
            Go {
                seen,
                argv: t3cli::do_it_argv(&title, &base, &branch, &folder),
                stdin: brief::do_it(&reference, &here, &all, &branch, &base, &nonce),
                title,
                branch: Some((branch, base)),
            }
        }
        (Mode::DoIt, _, _) => {
            let title = t3cli::thread_title(task.id, &task.title, None);
            Go {
                seen,
                argv: t3cli::folder_argv(&title, &folder),
                stdin: brief::research(&reference, &folder, &nonce),
                title,
                branch: None,
            }
        }
        (Mode::PlanIt, _, _) => {
            let repos: Vec<RepoPlace> = r
                .facts
                .task_repos()
                .0
                .into_iter()
                .map(|x| RepoPlace {
                    name: x.name,
                    path: x.path,
                })
                .collect();
            let title = t3cli::plan_title(task.id, &task.title);
            Go {
                seen,
                argv: t3cli::folder_argv(&title, &folder),
                stdin: brief::plan_it(&reference, &repos, &nonce),
                title,
                branch: None,
            }
        }
    };
    Ok(Planned::Go(go))
}

/// Exactly what `start` would send, place by place. Places that already started are left
/// out; the first place that would fail is the error.
pub(crate) fn briefs(inner: &Inner, req: &StartRequest) -> Result<Vec<Brief>, ServiceError> {
    let r = request(inner, req)?;
    let mut out = Vec::new();
    for spot in &r.spots {
        match plan(inner, &r, spot, req)? {
            Planned::Skip => {}
            Planned::Fail(e) => return Err(e),
            Planned::Go(go) => out.push(Brief {
                repo: spot.repo.as_ref().map(|x| x.id),
                argv: go.argv,
                stdin: go.stdin,
            }),
        }
    }
    Ok(out)
}

/// `LaunchService::start`.
pub(crate) fn start(inner: &Inner, req: &StartRequest) -> Result<LaunchReport, ServiceError> {
    if inner.shut.load(Ordering::SeqCst) {
        return Err(errors::shutting_down());
    }
    // Settle first, so a thread found since the last pass counts as started.
    settle::settle_task(inner, req.task, inner.clock.now())?;
    let r = request(inner, req)?;
    let task = r.facts.task.id;
    if let Some(name) = &r.branch
        && r.facts.task.branch.is_none()
    {
        inner
            .core
            .set_branch(task, name)
            .map_err(|e| core_error(task, &e))?;
    }
    let mut report = LaunchReport {
        started: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
        branch: r.branch.clone(),
        moved_to_doing: false,
    };
    let mut stopped: Option<ServiceError> = None;
    for spot in &r.spots {
        let repo = spot.repo.as_ref().map(|x| x.id);
        if let Some(e) = &stopped {
            report.failed.push((repo, e.clone()));
            continue;
        }
        let go = match plan(inner, &r, spot, req) {
            Ok(Planned::Skip) => {
                report.skipped.extend(repo);
                continue;
            }
            Ok(Planned::Fail(e)) | Err(e) => {
                report.failed.push((repo, e));
                continue;
            }
            Ok(Planned::Go(go)) => go,
        };
        match run(inner, task, req, spot, &go) {
            Ok(Ran::Started(thread)) => report.started.push(thread),
            Ok(Ran::Skipped) => report.skipped.extend(repo),
            Err(e) => {
                if t3cli::stops_the_loop(e.code) {
                    stopped = Some(e.clone());
                }
                if e.code == ErrorCode::T3Missing {
                    inner.queue(LaunchMsg::CheckAvailability);
                }
                report.failed.push((repo, e));
            }
        }
    }
    if !report.started.is_empty() {
        report.moved_to_doing = settle::move_to_doing(inner, task);
        inner.changes.bump();
    }
    Ok(report)
}

/// This call is running `t3` for a place; released when dropped.
struct InFlight<'a> {
    inner: &'a Inner,
    key: PlaceKey,
}

impl<'a> InFlight<'a> {
    /// `None` if another call in this process holds the place.
    fn claim(inner: &'a Inner, key: PlaceKey) -> Option<InFlight<'a>> {
        // Not `then_some`: it would build (and drop, releasing the other call's claim) an
        // `InFlight` even when the insert fails.
        let claimed = lock(&inner.in_flight).insert(key);
        if claimed {
            Some(InFlight { inner, key })
        } else {
            None
        }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        lock(&self.inner.in_flight).remove(&self.key);
    }
}

/// What one place came to.
enum Ran {
    Started(StartedThread),
    /// Another call started it first.
    Skipped,
}

/// Commits the pending row, runs `t3`, and settles the row by its answer.
fn run(
    inner: &Inner,
    task: TaskId,
    req: &StartRequest,
    spot: &Spot,
    go: &Go,
) -> Result<Ran, ServiceError> {
    let repo = spot.repo.as_ref().map(|x| x.id);
    let folder = spot.folder.display().to_string();
    let new = NewRow {
        task,
        repo,
        mode: req.mode,
        project_path: &folder,
        branch: go
            .branch
            .as_ref()
            .map(|(b, base)| (b.as_str(), base.as_str())),
        title: &go.title,
    };
    let Some(_flying) = InFlight::claim(inner, (task, repo, req.mode)) else {
        let name = spot.repo.as_ref().map(|x| x.name.as_str());
        return Err(errors::already_starting(name));
    };
    let inserted = lock(&inner.db).insert_pending(&new, req.again, go.seen, inner.clock.now())?;
    let id = match inserted {
        Inserted::Row(id) => id,
        // Another call got here first, between this call's check and now.
        Inserted::Started => return Ok(Ran::Skipped),
        Inserted::Pending => {
            let name = spot.repo.as_ref().map(|x| x.name.as_str());
            return Err(errors::already_starting(name));
        }
    };
    let result = lock(&inner.t3).run(&go.argv, Some(&go.stdin), inner.cfg.timeout);
    let answer = t3cli::read_start(result, &spot.label(), inner.cfg.timeout);
    let now = inner.clock.now();
    match answer {
        Answer::Started {
            thread_id,
            project_id,
        } => {
            let Some(row) = lock(&inner.db).started(id, &thread_id, now)? else {
                log(&format!(
                    "t3 answered with thread {thread_id}, which another row already has"
                ));
                return Err(errors::bad_answer());
            };
            if let (Some(repo), Some(project)) = (repo, project_id) {
                lock(&inner.db).set_t3_project(repo, &project, now)?;
            }
            let linked = settle::link(inner, &row);
            let mut thread = started_thread(&row).ok_or_else(errors::bad_answer)?;
            thread.linked = linked;
            Ok(Ran::Started(thread))
        }
        Answer::Failed(e) => {
            lock(&inner.db).failed(id, &e.message, now)?;
            Err(e)
        }
        // The row stays pending: a thread may exist, and settling will find it.
        Answer::Unsure(e) => Err(e),
    }
}

/// Keeps `NewLink` in one place: every thread link is a `t3_thread` with its title.
pub(crate) fn thread_link(thread: &str, title: &str) -> NewLink {
    NewLink::new(LinkKind::T3Thread, thread).with_title(title)
}

/// `LaunchService::add_to_t3`: `<t3> project add --title <name> <path>`, only ever from the
/// "Add to T3 and retry" button.
pub(crate) fn add_to_t3(inner: &Inner, repo: RepoId) -> Result<(), ServiceError> {
    if inner.shut.load(Ordering::SeqCst) {
        return Err(errors::shutting_down());
    }
    let repos = inner
        .core
        .read(|s| s.repos())
        .map_err(|e| errors::core(&e))?;
    let repo = repos
        .into_iter()
        .find(|r| r.id == repo)
        .ok_or_else(errors::repo_not_found)?;
    let path = PathBuf::from(&repo.path);
    if !path.is_absolute() {
        return Err(errors::not_a_full_path(&repo.name, &path));
    }
    if !path.is_dir() {
        return Err(errors::repo_missing(&repo.name, &path));
    }
    let argv = t3cli::project_add_argv(&repo.name, &repo.path);
    let result = lock(&inner.t3).run(&argv, None, inner.cfg.timeout);
    t3cli::read_other(result, &repo.name, inner.cfg.timeout).map(|_| ())
}
