//! The `git` checks Do it needs: the base branch, whether a branch exists, and git's own
//! branch-name rule.

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
}

/// The longest any one `git` call may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);

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
                child_env(std::env::vars_os()),
            ),
        }
    }

    fn git(&self, repo: Option<&Path>, args: &[&str]) -> io::Result<(Option<i32>, String)> {
        let mut argv = Vec::new();
        if let Some(repo) = repo {
            argv.push("-C".to_string());
            argv.push(repo.display().to_string());
        }
        argv.extend(args.iter().map(|a| a.to_string()));
        let out = self.runner.run(&argv, None, GIT_TIMEOUT)?;
        Ok((out.status, out.stdout.trim().to_string()))
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
}
