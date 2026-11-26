//! The `git` checks Do it needs: the base branch, whether a branch exists, and git's own
//! branch-name rule. For Herdr, which has no worktree command, also listing and adding
//! worktrees.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::process::{ProcessRunner, child_env};

/// `git`, for one repo at a time. The real one runs `/usr/bin/git`; tests use `FakeGit`.
pub trait Git: Send {
    /// `origin/HEAD` without the remote (`origin/main` gives `main`), else the current
    /// branch, else `None`.
    fn default_branch(&mut self, repo: &Path) -> io::Result<Option<String>>;
    /// Whether `refs/heads/<branch>` exists in the repo.
    fn has_branch(&mut self, repo: &Path, branch: &str) -> io::Result<bool>;
    /// `git check-ref-format --branch <name>`.
    fn valid_branch_name(&mut self, name: &str) -> io::Result<bool>;
    /// `git worktree list --porcelain`: every worktree of the repo, the main one included.
    fn worktrees(&mut self, repo: &Path) -> io::Result<Vec<Worktree>>;
    /// `git worktree add`: a new worktree at `path` on `branch`. With `start`, the branch is
    /// new and made from it (`-b <branch> <path> <start>`); without, the branch exists and is
    /// checked out. An `Err` carries git's own message.
    fn add_worktree(
        &mut self,
        repo: &Path,
        path: &Path,
        branch: &str,
        start: Option<&str>,
    ) -> io::Result<()>;
    /// `git -C <worktree> rev-parse --verify --quiet HEAD` succeeds.
    fn has_head(&mut self, worktree: &Path) -> io::Result<bool>;
    /// `git worktree remove -f -f <path>`: also a locked one, or one whose folder is gone.
    fn remove_worktree(&mut self, repo: &Path, path: &Path) -> io::Result<()>;
}

/// One worktree from `git worktree list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// The branch checked out, without `refs/heads/`; `None` when detached or bare.
    pub branch: Option<String>,
    /// Why it's locked. `git worktree add` locks a new worktree as `initializing` until its
    /// checkout is done, so a killed add leaves that lock behind.
    pub locked: Option<String>,
    /// Its folder is gone.
    pub prunable: bool,
}

impl Worktree {
    /// Left half-made by a `git worktree add` that didn't finish, or gone.
    pub fn unfinished(&self) -> bool {
        self.prunable || self.locked.as_deref() == Some(INITIALIZING)
    }
}

/// The lock reason `git worktree add` writes until its checkout is done (in the C locale,
/// which [`RealGit`] runs git in).
pub const INITIALIZING: &str = "initializing";

/// Reads `git worktree list --porcelain`.
pub fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            out.extend(current.take());
            current = Some(Worktree {
                path: PathBuf::from(path),
                branch: None,
                locked: None,
                prunable: false,
            });
        } else if let Some(wt) = current.as_mut() {
            if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                wt.branch = Some(branch.to_string());
            } else if line == "locked" {
                wt.locked = Some(String::new());
            } else if let Some(reason) = line.strip_prefix("locked ") {
                wt.locked = Some(reason.to_string());
            } else if line == "prunable" || line.starts_with("prunable ") {
                wt.prunable = true;
            }
        }
    }
    out.extend(current);
    out
}

/// The longest any one `git` call may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest `git worktree add` may take: it checks out the whole tree.
const WORKTREE_TIMEOUT: Duration = Duration::from_secs(120);

/// `/usr/bin/git`, run with argv only from `home`.
pub struct RealGit {
    runner: ProcessRunner,
}

impl RealGit {
    pub fn new(home: &Path) -> RealGit {
        RealGit {
            runner: ProcessRunner::new(
                PathBuf::from("/usr/bin/git"),
                home.to_path_buf(),
                // The C locale, so the lock reason `initializing` reads as it was written.
                child_env(std::env::vars_os())
                    .into_iter()
                    .filter(|(k, _)| k != "LC_ALL")
                    .chain([("LC_ALL".into(), "C".into())])
                    .collect(),
            ),
        }
    }

    fn git(&self, repo: Option<&Path>, args: &[&str]) -> io::Result<(Option<i32>, String)> {
        let out = self.run(repo, args, GIT_TIMEOUT)?;
        Ok((out.status, out.stdout.trim().to_string()))
    }

    fn run(
        &self,
        repo: Option<&Path>,
        args: &[&str],
        timeout: Duration,
    ) -> io::Result<crate::process::CmdOutput> {
        let mut argv = Vec::new();
        if let Some(repo) = repo {
            argv.push("-C".to_string());
            argv.push(repo.display().to_string());
        }
        argv.extend(args.iter().map(|a| a.to_string()));
        self.runner.run(&argv, None, timeout)
    }
}

impl Git for RealGit {
    fn default_branch(&mut self, repo: &Path) -> io::Result<Option<String>> {
        let (status, out) = self.git(
            Some(repo),
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )?;
        if status == Some(0)
            && let Some((_, branch)) = out.split_once('/')
            && !branch.is_empty()
        {
            return Ok(Some(branch.to_string()));
        }
        let (status, out) = self.git(Some(repo), &["branch", "--show-current"])?;
        Ok((status == Some(0) && !out.is_empty()).then_some(out))
    }

    fn has_branch(&mut self, repo: &Path, branch: &str) -> io::Result<bool> {
        let r = format!("refs/heads/{branch}");
        match self.git(Some(repo), &["rev-parse", "--verify", "--quiet", &r])? {
            (Some(0), _) => Ok(true),
            (Some(1), _) => Ok(false),
            (status, _) => Err(io::Error::other(format!(
                "git rev-parse ended with {status:?}"
            ))),
        }
    }

    fn valid_branch_name(&mut self, name: &str) -> io::Result<bool> {
        let (status, _) = self.git(None, &["check-ref-format", "--branch", name])?;
        Ok(status == Some(0))
    }

    fn worktrees(&mut self, repo: &Path) -> io::Result<Vec<Worktree>> {
        let out = self.run(
            Some(repo),
            &["worktree", "list", "--porcelain"],
            GIT_TIMEOUT,
        )?;
        if out.status != Some(0) {
            return Err(io::Error::other(first_line(
                &out.stderr,
                "git worktree list failed",
            )));
        }
        Ok(parse_worktrees(&out.stdout))
    }

    fn add_worktree(
        &mut self,
        repo: &Path,
        path: &Path,
        branch: &str,
        start: Option<&str>,
    ) -> io::Result<()> {
        let path = path.display().to_string();
        let mut args = vec!["worktree", "add"];
        match start {
            Some(start) => args.extend(["-b", branch, &path, start]),
            None => args.extend([path.as_str(), branch]),
        }
        let out = self.run(Some(repo), &args, WORKTREE_TIMEOUT)?;
        if out.status == Some(0) {
            Ok(())
        } else {
            Err(io::Error::other(first_line(
                &out.stderr,
                "git worktree add failed",
            )))
        }
    }

    fn has_head(&mut self, worktree: &Path) -> io::Result<bool> {
        let (status, _) = self.git(
            Some(worktree),
            &["rev-parse", "--verify", "--quiet", "HEAD"],
        )?;
        Ok(status == Some(0))
    }

    fn remove_worktree(&mut self, repo: &Path, path: &Path) -> io::Result<()> {
        let path = path.display().to_string();
        let out = self.run(
            Some(repo),
            &["worktree", "remove", "-f", "-f", &path],
            WORKTREE_TIMEOUT,
        )?;
        if out.status == Some(0) {
            Ok(())
        } else {
            Err(io::Error::other(first_line(
                &out.stderr,
                "git worktree remove failed",
            )))
        }
    }
}

/// The first line of `text` that isn't empty, or `otherwise`.
fn first_line(text: &str, otherwise: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or(otherwise)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt(path: &str, branch: Option<&str>, locked: Option<&str>, prunable: bool) -> Worktree {
        Worktree {
            path: path.into(),
            branch: branch.map(str::to_string),
            locked: locked.map(str::to_string),
            prunable,
        }
    }

    #[test]
    fn porcelain_lists_paths_branches_and_unfinished_ones() {
        let text = "worktree /src/web\nHEAD 1234\nbranch refs/heads/main\n\n\
                    worktree /wt/web/task-1-x\nHEAD 5678\nbranch refs/heads/task-1/x\n\n\
                    worktree /wt/web/old\nHEAD 9abc\ndetached\nlocked\n\n\
                    worktree /wt/web/half\nHEAD 9abc\nbranch refs/heads/h\nlocked initializing\n\n\
                    worktree /wt/web/gone\nHEAD 9abc\nbranch refs/heads/g\n\
                    prunable gitdir file points to non-existent location\n";
        let all = parse_worktrees(text);
        assert_eq!(
            all,
            [
                wt("/src/web", Some("main"), None, false),
                wt("/wt/web/task-1-x", Some("task-1/x"), None, false),
                wt("/wt/web/old", None, Some(""), false),
                wt("/wt/web/half", Some("h"), Some("initializing"), false),
                wt("/wt/web/gone", Some("g"), None, true),
            ]
        );
        let unfinished: Vec<bool> = all.iter().map(Worktree::unfinished).collect();
        assert_eq!(unfinished, [false, false, false, true, true]);
        assert!(parse_worktrees("").is_empty());
    }
}
