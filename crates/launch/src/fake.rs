//! Fakes of `t3`, `git` and Herdr for tests, behind the `fakes` feature. None runs a
//! process, so no test can start a real T3 thread or Herdr tab.
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

use serde_json::{Value, json};

use crate::git::{Git, Worktree};
use crate::herdr::{HerdrCli, Tool};
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
    worktrees: HashMap<PathBuf, Vec<Worktree>>,
    refuse_worktree: VecDeque<String>,
    no_head: HashSet<PathBuf>,
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

    /// Makes the next `worktree add` fail with git's `message`. Queued in order.
    pub fn refuse_worktree(&self, message: &str) {
        lock(&self.0).refuse_worktree.push_back(message.to_string());
    }

    /// Makes `path` a worktree of `repo` on `branch` (or detached).
    pub fn add_worktree_at(&self, repo: impl Into<PathBuf>, path: &Path, branch: Option<&str>) {
        self.add(repo.into(), path, branch, None);
    }

    /// Makes `path` a worktree of `repo` on `branch` that a killed `git worktree add` left:
    /// still locked as `initializing`.
    pub fn half_made_worktree_at(&self, repo: impl Into<PathBuf>, path: &Path, branch: &str) {
        let repo = repo.into();
        lock(&self.0)
            .branches
            .insert((repo.clone(), branch.to_string()));
        self.add(repo, path, Some(branch), Some(crate::git::INITIALIZING));
    }

    /// Makes `rev-parse HEAD` fail in the worktree at `path`.
    pub fn no_head_in(&self, path: &Path) {
        lock(&self.0).no_head.insert(path.to_path_buf());
    }

    fn add(&self, repo: PathBuf, path: &Path, branch: Option<&str>, locked: Option<&str>) {
        lock(&self.0)
            .worktrees
            .entry(repo)
            .or_default()
            .push(Worktree {
                path: path.to_path_buf(),
                branch: branch.map(str::to_string),
                locked: locked.map(str::to_string),
                prunable: false,
            });
    }

    /// The worktrees made or added for `repo` so far, in order.
    pub fn worktrees_of(&self, repo: &Path) -> Vec<Worktree> {
        lock(&self.0)
            .worktrees
            .get(repo)
            .cloned()
            .unwrap_or_default()
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

    fn worktrees(&mut self, repo: &Path) -> io::Result<Vec<Worktree>> {
        let s = self.call(format!("worktrees({})", repo.display()))?;
        Ok(s.worktrees.get(repo).cloned().unwrap_or_default())
    }

    /// Like git: refuses a branch checked out in another worktree, a new branch that
    /// exists, or an existing branch that doesn't; makes the folder (in the test's temp dir).
    fn add_worktree(
        &mut self,
        repo: &Path,
        path: &Path,
        branch: &str,
        start: Option<&str>,
    ) -> io::Result<()> {
        let mut s = self.call(format!(
            "add_worktree({}, {}, {branch}, {})",
            repo.display(),
            path.display(),
            start.unwrap_or("-")
        ))?;
        if let Some(message) = s.refuse_worktree.pop_front() {
            return Err(io::Error::other(message));
        }
        let key = (repo.to_path_buf(), branch.to_string());
        let exists = s.branches.contains(&key);
        if start.is_some() && exists {
            return Err(io::Error::other(format!(
                "fatal: a branch named '{branch}' already exists"
            )));
        }
        if start.is_none() && !exists {
            return Err(io::Error::other(format!(
                "fatal: invalid reference: {branch}"
            )));
        }
        let used = s
            .worktrees
            .get(repo)
            .into_iter()
            .flatten()
            .any(|w| w.branch.as_deref() == Some(branch));
        if used {
            return Err(io::Error::other(format!(
                "fatal: '{branch}' is already used by a worktree"
            )));
        }
        std::fs::create_dir_all(path)?;
        s.branches.insert(key);
        s.worktrees
            .entry(repo.to_path_buf())
            .or_default()
            .push(Worktree {
                path: path.to_path_buf(),
                branch: Some(branch.to_string()),
                locked: None,
                prunable: false,
            });
        Ok(())
    }

    fn has_head(&mut self, worktree: &Path) -> io::Result<bool> {
        let s = self.call(format!("has_head({})", worktree.display()))?;
        Ok(!s.no_head.contains(worktree))
    }

    fn remove_worktree(&mut self, repo: &Path, path: &Path) -> io::Result<()> {
        let mut s = self.call(format!(
            "remove_worktree({}, {})",
            repo.display(),
            path.display()
        ))?;
        s.no_head.remove(path);
        if let Some(list) = s.worktrees.get_mut(repo) {
            list.retain(|w| w.path != path);
        }
        Ok(())
    }
}

/// One call made to [`FakeHerdr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrCall {
    pub tool: Tool,
    pub argv: Vec<String>,
}

impl HerdrCall {
    /// `herdr-app do <command> …`: the command's name.
    pub fn command(&self) -> Option<&str> {
        (self.tool == Tool::App && self.argv.first().is_some_and(|a| a == "do"))
            .then(|| self.argv.get(1).map(String::as_str))
            .flatten()
    }

    /// The JSON object `herdr-app do` was given.
    pub fn args(&self) -> Option<Value> {
        self.command()?;
        serde_json::from_str(self.argv.get(2)?).ok()
    }
}

/// A tab the fake Herdr opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeTab {
    pub space: String,
    pub name: String,
    pub folder: String,
    pub argv: Vec<String>,
    pub task: String,
    pub focus: Option<bool>,
    /// Its terminal's address.
    pub terminal: String,
    /// Made by `space.new` rather than `tab.new`.
    pub new_space: bool,
}

/// How the fake `herdr-app do` answers one `tab.new` or `space.new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrReply {
    /// Exit 1 with `{"error": message}`.
    Error(String),
    /// Exit 0 with this stdout.
    Stdout(String),
    /// No answer in time.
    Timeout,
    /// Opens the tab, then gives no answer in time: a tab Airboss never hears of.
    OpensThenTimeout,
}

#[derive(Default)]
struct FakeSpace {
    id: String,
    name: Option<String>,
    folder: String,
    machine: Option<String>,
}

type HerdrHook = Box<dyn FnMut(&HerdrCall) + Send>;

struct HerdrState {
    installed: bool,
    hc_installed: bool,
    window: bool,
    /// What `open -b` does to the window: opens it after this many `status` calls, or never.
    opens_after: Option<u32>,
    statuses_left: Option<u32>,
    spaces: Vec<FakeSpace>,
    tabs: Vec<FakeTab>,
    replies: VecDeque<HerdrReply>,
    labels: HashMap<String, Vec<(String, String)>>,
    /// Each terminal's launch folder and `created_ms`.
    started: HashMap<String, (String, u64)>,
    now_ms: u64,
    finished: HashSet<String>,
    hc_broken: bool,
    label_fails: bool,
    calls: Vec<HerdrCall>,
    pauses: Vec<Duration>,
    next: u32,
    hook: Option<HerdrHook>,
}

impl Default for HerdrState {
    fn default() -> HerdrState {
        HerdrState {
            installed: true,
            hc_installed: true,
            window: true,
            opens_after: Some(1),
            statuses_left: None,
            spaces: Vec::new(),
            tabs: Vec::new(),
            replies: VecDeque::new(),
            labels: HashMap::new(),
            started: HashMap::new(),
            now_ms: 0,
            finished: HashSet::new(),
            hc_broken: false,
            label_fails: false,
            calls: Vec::new(),
            pauses: Vec::new(),
            next: 0,
            hook: None,
        }
    }
}

/// A scripted Herdr: `herdr-app` with an open window and no spaces, and `hc` holding the
/// labels of the terminals it "started". Clones share one state.
#[derive(Clone, Default)]
pub struct FakeHerdr(Arc<Mutex<HerdrState>>);

impl FakeHerdr {
    pub fn new() -> FakeHerdr {
        FakeHerdr::default()
    }

    /// Neither `herdr-app` nor `hc` is installed.
    pub fn uninstall(&self) {
        let mut s = lock(&self.0);
        s.installed = false;
        s.hc_installed = false;
    }

    /// `herdr-app` is installed but `hc` isn't.
    pub fn without_hc(&self) {
        lock(&self.0).hc_installed = false;
    }

    /// The window is closed. `open -b` opens it, answering the second `status` after it.
    pub fn close_window(&self) {
        lock(&self.0).window = false;
    }

    /// The window is closed, and `open -b` never opens it.
    pub fn never_opens(&self) {
        let mut s = lock(&self.0);
        s.window = false;
        s.opens_after = None;
    }

    /// A space on this computer (`machine: None`) or another.
    pub fn add_space(&self, id: &str, name: Option<&str>, folder: &Path, machine: Option<&str>) {
        lock(&self.0).spaces.push(FakeSpace {
            id: id.to_string(),
            name: name.map(str::to_string),
            folder: folder.display().to_string(),
            machine: machine.map(str::to_string),
        });
    }

    /// How the next `tab.new` or `space.new` with an agent answers instead of opening its
    /// tab. Queued.
    pub fn push(&self, reply: HerdrReply) {
        lock(&self.0).replies.push_back(reply);
    }

    /// A terminal Airboss didn't start, carrying these labels (to settle against).
    pub fn add_terminal(&self, address: &str, labels: &[(&str, &str)]) {
        lock(&self.0).labels.insert(
            address.to_string(),
            labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
    }

    /// A terminal Airboss didn't start, in `folder`, made at `created_ms`, with these labels.
    pub fn add_terminal_in(
        &self,
        address: &str,
        folder: &Path,
        created_ms: u64,
        labels: &[(&str, &str)],
    ) {
        self.add_terminal(address, labels);
        lock(&self.0).started.insert(
            address.to_string(),
            (folder.display().to_string(), created_ms),
        );
    }

    /// The `created_ms` new terminals get from now on.
    pub fn set_now_ms(&self, ms: u64) {
        lock(&self.0).now_ms = ms;
    }

    /// The terminal's program ended: `hc ls` no longer lists it.
    pub fn finish(&self, address: &str) {
        lock(&self.0).finished.insert(address.to_string());
    }

    /// Every `hc` call fails.
    pub fn break_hc(&self) {
        lock(&self.0).hc_broken = true;
    }

    /// `hc label` fails.
    pub fn fail_labels(&self) {
        lock(&self.0).label_fails = true;
    }

    /// Runs on every call, before the answer, with no lock held.
    pub fn on_call(&self, hook: impl FnMut(&HerdrCall) + Send + 'static) {
        lock(&self.0).hook = Some(Box::new(hook));
    }

    pub fn calls(&self) -> Vec<HerdrCall> {
        lock(&self.0).calls.clone()
    }

    /// Every tab opened so far, in order.
    pub fn tabs(&self) -> Vec<FakeTab> {
        lock(&self.0).tabs.clone()
    }

    /// A terminal's labels, as `key=value`, sorted.
    pub fn labels(&self, address: &str) -> Vec<String> {
        let s = lock(&self.0);
        let mut out: Vec<String> = s
            .labels
            .get(address)
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        out.sort();
        out
    }

    /// Every pause taken while waiting for the window.
    pub fn pauses(&self) -> Vec<Duration> {
        lock(&self.0).pauses.clone()
    }

    /// The ids and names of the spaces it has.
    pub fn spaces(&self) -> Vec<(String, Option<String>, String)> {
        lock(&self.0)
            .spaces
            .iter()
            .map(|s| (s.id.clone(), s.name.clone(), s.folder.clone()))
            .collect()
    }
}

fn exit(code: i32, stdout: String, stderr: &str) -> io::Result<CmdOutput> {
    Ok(CmdOutput {
        status: Some(code),
        stdout,
        stderr: stderr.to_string(),
    })
}

impl HerdrState {
    fn app(&mut self, argv: &[String]) -> io::Result<CmdOutput> {
        if !self.installed {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        if args == ["--version"] {
            return exit(0, "herdr-app 0.1.18\n".into(), "");
        }
        if !self.window {
            if args == ["status"]
                && let Some(left) = self.statuses_left.as_mut()
            {
                if *left == 0 {
                    self.window = true;
                } else {
                    *left -= 1;
                }
            }
            if !self.window {
                return exit(
                    2,
                    String::new(),
                    "herdr-app: no Herdr window at /fake/herdr-app.sock: No such file or directory\n",
                );
            }
        }
        match args.as_slice() {
            ["status"] => exit(0, "{\"errors\": []}\n".into(), ""),
            ["ls"] => {
                let spaces: Vec<Value> = self
                    .spaces
                    .iter()
                    .map(|s| {
                        json!({"id": s.id, "name": s.name, "folder": s.folder,
                               "machine": s.machine, "active": false, "tabs": []})
                    })
                    .collect();
                exit(0, json!({"spaces": spaces, "recent": []}).to_string(), "")
            }
            ["do", cmd @ ("tab.new" | "space.new"), json] => self.open(cmd, json),
            _ => exit(
                1,
                json!({"error": format!("fake herdr-app: {argv:?}")}).to_string(),
                "",
            ),
        }
    }

    fn open(&mut self, cmd: &str, json: &str) -> io::Result<CmdOutput> {
        let refuse = |m: String| exit(1, json!({ "error": m }).to_string(), "");
        let Ok(args) = serde_json::from_str::<Value>(json) else {
            return refuse("fake herdr-app: not one JSON object".into());
        };
        // Scripted replies are for tabs that start an agent, not a space's shell tab.
        let reply = if args.get("argv").is_some() {
            self.replies.pop_front()
        } else {
            None
        };
        let lost_answer = match reply {
            Some(HerdrReply::Error(m)) => return refuse(m),
            Some(HerdrReply::Stdout(text)) => return exit(0, text, ""),
            Some(HerdrReply::Timeout) => return Err(io::Error::from(io::ErrorKind::TimedOut)),
            Some(HerdrReply::OpensThenTimeout) => true,
            None => false,
        };
        let text = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
        let folder = text("folder").unwrap_or_default();
        if !Path::new(&folder).is_dir() {
            return refuse(format!("cannot start a terminal: no folder {folder}"));
        }
        self.next += 1;
        let space = if cmd == "space.new" {
            let id = format!("sp{}", self.next);
            self.spaces.push(FakeSpace {
                id: id.clone(),
                name: text("name"),
                folder: folder.clone(),
                machine: None,
            });
            id
        } else {
            let id = text("space").unwrap_or_default();
            if !self.spaces.iter().any(|s| s.id == id) {
                return refuse(format!("no space \"{id}\""));
            }
            id
        };
        let terminal = format!("t{}", self.next);
        self.tabs.push(FakeTab {
            space: space.clone(),
            name: text(if cmd == "space.new" { "tab" } else { "name" }).unwrap_or_default(),
            folder,
            argv: args
                .get("argv")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            task: text("task").unwrap_or_default(),
            focus: args.get("focus").and_then(Value::as_bool),
            terminal: terminal.clone(),
            new_space: cmd == "space.new",
        });
        self.labels.entry(terminal.clone()).or_default();
        let made = (folder_of_last(&self.tabs), self.now_ms);
        self.started.insert(terminal.clone(), made);
        if lost_answer {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        let out = json!({"space": space, "tab": format!("tb{}", self.next), "terminal": terminal});
        exit(0, format!("{out:#}\n"), "")
    }

    fn hc(&mut self, argv: &[String]) -> io::Result<CmdOutput> {
        if !self.hc_installed {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        if args == ["--version"] {
            return exit(0, "hc 0.1.22\n".into(), "");
        }
        if self.hc_broken {
            return exit(1, String::new(), "hc: the core isn't answering\n");
        }
        match args.as_slice() {
            ["label", address, pairs @ ..] => {
                if self.label_fails || !self.labels.contains_key(*address) {
                    return exit(3, json!({"error": {"kind": "not_found"}}).to_string(), "");
                }
                let labels = self.labels.entry(address.to_string()).or_default();
                for pair in pairs {
                    if let Some((k, v)) = pair.split_once('=') {
                        labels.retain(|(key, _)| key != k);
                        labels.push((k.to_string(), v.to_string()));
                    }
                }
                exit(0, json!({"address": address}).to_string(), "")
            }
            ["get", address, "--json"] => {
                let Some(labels) = self.labels.get(*address) else {
                    return exit(3, json!({"error": {"kind": "not_found"}}).to_string(), "");
                };
                let (cwd, created) = self.started.get(*address).cloned().unwrap_or_default();
                let labels: serde_json::Map<String, Value> = labels
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::from(v.as_str())))
                    .collect();
                let state = if self.finished.contains(*address) {
                    "finished"
                } else {
                    "live"
                };
                let out = json!({"address": address, "cwd": cwd, "created_ms": created,
                                 "state": state, "labels": labels});
                exit(0, out.to_string(), "")
            }
            ["ls", "--json", rest @ ..] => {
                let wanted: Vec<(&str, &str)> = rest
                    .chunks(2)
                    .filter_map(|c| match c {
                        ["--label", kv] => kv.split_once('='),
                        _ => None,
                    })
                    .collect();
                let mut found: Vec<&String> = self
                    .labels
                    .iter()
                    .filter(|(a, _)| !self.finished.contains(*a))
                    .filter(|(_, have)| {
                        wanted
                            .iter()
                            .all(|(k, v)| have.iter().any(|(hk, hv)| hk == k && hv == v))
                    })
                    .map(|(a, _)| a)
                    .collect();
                found.sort();
                let terminals: Vec<Value> = found
                    .into_iter()
                    .map(|a| {
                        let cwd = self.started.get(a).map(|(c, _)| c.as_str()).unwrap_or("/");
                        json!({"address": a, "cwd": cwd, "state": "live"})
                    })
                    .collect();
                exit(0, json!({"terminals": terminals}).to_string(), "")
            }
            _ => exit(2, String::new(), &format!("fake hc: unexpected {argv:?}")),
        }
    }
}

impl HerdrCli for FakeHerdr {
    fn run(&mut self, tool: Tool, argv: &[String], _timeout: Duration) -> io::Result<CmdOutput> {
        let call = HerdrCall {
            tool,
            argv: argv.to_vec(),
        };
        let mut hook = {
            let mut s = lock(&self.0);
            s.calls.push(call.clone());
            s.hook.take()
        };
        if let Some(h) = hook.as_mut() {
            h(&call);
        }
        let mut s = lock(&self.0);
        if s.hook.is_none() {
            s.hook = hook;
        }
        match tool {
            Tool::App => s.app(argv),
            Tool::Hc => s.hc(argv),
            Tool::Open => {
                if argv != ["-g", "-b", "dev.herdr.app"] {
                    return exit(1, String::new(), "fake open: unexpected argv");
                }
                if !s.installed {
                    return exit(1, String::new(), "Unable to find application dev.herdr.app");
                }
                if !s.window {
                    s.statuses_left = s.opens_after;
                }
                exit(0, String::new(), "")
            }
        }
    }

    fn pause(&mut self, d: Duration) {
        lock(&self.0).pauses.push(d);
    }
}

/// The folder of the tab just opened.
fn folder_of_last(tabs: &[FakeTab]) -> String {
    tabs.last().map(|t| t.folder.clone()).unwrap_or_default()
}
