//! Fakes of `t3` and `git` for tests, behind the `fakes` feature. Neither runs a process,
//! so no test can start a real T3 thread.
//!
//! [`FakeT3Cli`] is a recorded-behaviour runner: it checks each call's command and key
//! arguments, records argv and stdin, runs an optional hook at call time (to look at the
//! table while `t3` "runs"), and answers from a script.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::git::Git;
use crate::process::CmdOutput;
use crate::t3cli::T3Cli;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One call made to [`FakeT3Cli`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Call {
    pub argv: Vec<String>,
    pub stdin: Option<String>,
    pub timeout: Duration,
}

impl T3Call {
    /// `thread start …`.
    pub fn is_start(&self) -> bool {
        self.argv
            .starts_with(&["thread".to_string(), "start".to_string()])
    }

    /// The value after `--name`.
    pub fn flag(&self, name: &str) -> Option<&str> {
        let at = self.argv.iter().position(|a| a == name)?;
        self.argv.get(at + 1).map(String::as_str)
    }

    /// The project argument of `thread start` (the one before `-`).
    pub fn project(&self) -> Option<&str> {
        let n = self.argv.len();
        (self.is_start() && n >= 2).then(|| self.argv[n - 2].as_str())
    }
}

/// How the fake `t3` answers one call.
#[derive(Debug)]
pub enum Reply {
    /// Exit 0 with `{"threadId": id, "projectId": "project-1", "runId": null}`.
    Thread(String),
    /// Exit 0 with this stdout.
    Stdout(String),
    /// This exit code, with this line on stderr.
    Exit(i32, String),
    /// No answer in time (`ErrorKind::TimedOut`).
    Timeout,
    /// No binary (`ErrorKind::NotFound`).
    Missing,
    /// Ended by a signal.
    Killed,
    /// Says it's running on `entered`, waits for `release` (at most 10 s), then answers.
    Hold {
        entered: Sender<()>,
        release: Receiver<()>,
        then: Box<Reply>,
    },
}

/// How `t3 --version` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// Exit 0 with this stdout.
    Prints(String),
    /// This exit code and stderr line.
    Exit(i32, String),
    /// No binary.
    Missing,
}

impl Default for Version {
    fn default() -> Version {
        Version::Prints("t3 v0.1.12\n".to_string())
    }
}

type Hook = Box<dyn FnMut(&T3Call) + Send>;

#[derive(Default)]
struct T3State {
    starts: VecDeque<Reply>,
    others: VecDeque<Reply>,
    version: Version,
    calls: Vec<T3Call>,
    next_thread: u32,
    hook: Option<Hook>,
}

/// A scripted `t3`. Clones share one script and one call log.
#[derive(Clone, Default)]
pub struct FakeT3Cli(Arc<Mutex<T3State>>);

impl FakeT3Cli {
    /// Answers every `thread start` with a new thread (`thread-1`, `thread-2` …) and
    /// `--version` with `t3 v0.1.12`.
    pub fn new() -> FakeT3Cli {
        FakeT3Cli::default()
    }

    /// How the next unscripted `thread start` answers. Queued in order.
    pub fn push(&self, reply: Reply) {
        lock(&self.0).starts.push_back(reply);
    }

    /// How the next other command (`project add`) answers. Queued in order.
    pub fn push_other(&self, reply: Reply) {
        lock(&self.0).others.push_back(reply);
    }

    /// How every `--version` answers from now on.
    pub fn set_version(&self, version: Version) {
        lock(&self.0).version = version;
    }

    /// Runs on every call, before the answer, with no lock held.
    pub fn on_call(&self, hook: impl FnMut(&T3Call) + Send + 'static) {
        lock(&self.0).hook = Some(Box::new(hook));
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<T3Call> {
        lock(&self.0).calls.clone()
    }

    /// Every `thread start` so far, in order.
    pub fn starts(&self) -> Vec<T3Call> {
        self.calls().into_iter().filter(T3Call::is_start).collect()
    }

    fn answer(&self, reply: Reply) -> io::Result<CmdOutput> {
        let ok = |stdout: String| {
            Ok(CmdOutput {
                status: Some(0),
                stdout,
                stderr: String::new(),
            })
        };
        match reply {
            Reply::Thread(id) => ok(format!(
                "{{\"threadId\":\"{id}\",\"projectId\":\"project-1\",\"runId\":null}}\n"
            )),
            Reply::Stdout(text) => ok(text),
            Reply::Exit(code, stderr) => Ok(CmdOutput {
                status: Some(code),
                stdout: String::new(),
                stderr: format!("{stderr}\n"),
            }),
            Reply::Timeout => Err(io::Error::from(io::ErrorKind::TimedOut)),
            Reply::Missing => Err(io::Error::from(io::ErrorKind::NotFound)),
            Reply::Killed => Ok(CmdOutput::default()),
            Reply::Hold {
                entered,
                release,
                then,
            } => {
                let _ = entered.send(());
                let _ = release.recv_timeout(Duration::from_secs(10));
                self.answer(*then)
            }
        }
    }
}

impl T3Cli for FakeT3Cli {
    fn run(
        &mut self,
        argv: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        let call = T3Call {
            argv: argv.to_vec(),
            stdin: stdin.map(str::to_string),
            timeout,
        };
        let mut hook = {
            let mut s = lock(&self.0);
            s.calls.push(call.clone());
            s.hook.take()
        };
        if let Some(h) = hook.as_mut() {
            h(&call);
        }
        let reply = {
            let mut s = lock(&self.0);
            if s.hook.is_none() {
                s.hook = hook;
            }
            if argv == ["--version"] {
                match &s.version {
                    Version::Prints(text) => Reply::Stdout(text.clone()),
                    Version::Exit(code, text) => Reply::Exit(*code, text.clone()),
                    Version::Missing => Reply::Missing,
                }
            } else if call.is_start() {
                let well_formed = argv.get(2).is_some_and(|a| a == "--json")
                    && argv.last().is_some_and(|a| a == "-")
                    && stdin.is_some_and(|s| !s.trim().is_empty())
                    && call.flag("--title").is_some()
                    && call.project().is_some_and(|p| p.starts_with('/'));
                if !well_formed {
                    Reply::Exit(2, format!("fake t3: malformed thread start {argv:?}"))
                } else {
                    s.starts.pop_front().unwrap_or_else(|| {
                        s.next_thread += 1;
                        Reply::Thread(format!("thread-{}", s.next_thread))
                    })
                }
            } else if argv.starts_with(&["project".to_string(), "add".to_string()]) {
                s.others
                    .pop_front()
                    .unwrap_or_else(|| Reply::Stdout(String::new()))
            } else {
                Reply::Exit(2, format!("fake t3: unexpected command {argv:?}"))
            }
        };
        self.answer(reply)
    }
}

#[derive(Default)]
struct GitState {
    defaults: HashMap<PathBuf, Option<String>>,
    branches: HashSet<(PathBuf, String)>,
    invalid: HashSet<String>,
    broken: bool,
    calls: Vec<String>,
}

/// A scripted `git`. Every repo's default branch is `main` unless set; no branch exists
/// unless added; names follow a small part of git's rule unless refused. Clones share state.
#[derive(Clone, Default)]
pub struct FakeGit(Arc<Mutex<GitState>>);

impl FakeGit {
    pub fn new() -> FakeGit {
        FakeGit::default()
    }

    /// What `default_branch(repo)` answers.
    pub fn set_default(&self, repo: impl Into<PathBuf>, branch: Option<&str>) {
        lock(&self.0)
            .defaults
            .insert(repo.into(), branch.map(str::to_string));
    }

    /// Makes `branch` exist in `repo`.
    pub fn add_branch(&self, repo: impl Into<PathBuf>, branch: &str) {
        lock(&self.0)
            .branches
            .insert((repo.into(), branch.to_string()));
    }

    /// Makes `check-ref-format` refuse `name`.
    pub fn refuse_name(&self, name: &str) {
        lock(&self.0).invalid.insert(name.to_string());
    }

    /// Makes every call fail with an I/O error.
    pub fn break_it(&self) {
        lock(&self.0).broken = true;
    }

    /// Every call so far, e.g. `"default_branch(/src/web)"`.
    pub fn calls(&self) -> Vec<String> {
        lock(&self.0).calls.clone()
    }

    fn call(&self, what: String) -> io::Result<MutexGuard<'_, GitState>> {
        let mut s = lock(&self.0);
        s.calls.push(what);
        if s.broken {
            return Err(io::Error::other("fake git is broken"));
        }
        Ok(s)
    }
}

impl Git for FakeGit {
    fn default_branch(&mut self, repo: &Path) -> io::Result<Option<String>> {
        let s = self.call(format!("default_branch({})", repo.display()))?;
        Ok(s.defaults
            .get(repo)
            .cloned()
            .unwrap_or_else(|| Some("main".to_string())))
    }

    fn has_branch(&mut self, repo: &Path, branch: &str) -> io::Result<bool> {
        let s = self.call(format!("has_branch({}, {branch})", repo.display()))?;
        Ok(s.branches
            .contains(&(repo.to_path_buf(), branch.to_string())))
    }

    fn valid_branch_name(&mut self, name: &str) -> io::Result<bool> {
        let s = self.call(format!("valid_branch_name({name})"))?;
        let gitish = !name.contains("@{") && !name.contains("//") && !name.ends_with('.');
        Ok(gitish && !s.invalid.contains(name))
    }
}
