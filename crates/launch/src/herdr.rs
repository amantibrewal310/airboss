//! Everything about Herdr: `herdr-app` (the window: its spaces and tabs), `hc` (its core: the
//! terminals and their labels), and where Herdr keeps worktrees.
//!
//! Facts come from `herdr-app --skill`, `hc --skill`, `herdr-app commands` and probes in a
//! space of their own (SPEC-launch, "Herdr"). Every argument goes as argv with no shell, and
//! `herdr-app do` gets its arguments as one JSON object, so the brief is never parsed by a
//! shell or read as a flag.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use airboss_core::TaskId;
use airboss_services::{Availability, HerdrAgent, ServiceError};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::errors;
use crate::process::{CmdOutput, ProcessRunner, child_env};
use crate::t3cli::Answer;

/// Herdr.app's bundle id, for `open -b`.
pub const BUNDLE_ID: &str = "dev.herdr.app";

/// How long to wait for the window after opening Herdr.
pub const WINDOW_WAIT: Duration = Duration::from_secs(10);

/// The pause between two looks at the window while it opens.
pub const WINDOW_POLL: Duration = Duration::from_millis(500);

/// The longest `herdr-app status` may take.
const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

/// The longest `--version`, `hc label` or `hc ls` may take.
pub const QUICK_TIMEOUT: Duration = Duration::from_secs(10);

/// The program one Herdr call runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// `herdr-app`: the window.
    App,
    /// `hc`: the core.
    Hc,
    /// `/usr/bin/open`, to open Herdr.app when its window doesn't answer.
    Open,
}

/// Runs Herdr's programs. The real one is [`RealHerdr`]; tests use `FakeHerdr`.
pub trait HerdrCli: Send {
    /// Runs `tool` with `argv` and waits at most `timeout`. A program that isn't installed is
    /// `ErrorKind::NotFound`; running out of time is `ErrorKind::TimedOut`.
    fn run(&mut self, tool: Tool, argv: &[String], timeout: Duration) -> io::Result<CmdOutput>;
    /// Waits between two looks at the window while Herdr opens. Fakes return at once.
    fn pause(&mut self, d: Duration);
}

/// Environment variables a Herdr child never inherits, on top of [`crate::REMOVED_VARS`]:
/// a tab Airboss opens isn't the terminal Airboss itself may run in.
const HERDR_REMOVED_VARS: [&str; 1] = ["HC_TERMINAL_ID"];

/// The real programs: `~/.local/bin/herdr-app` and `~/.local/bin/hc`, else the first on
/// `PATH`; and `/usr/bin/open`. Each runs from `home`, with no shell.
pub struct RealHerdr {
    home: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl RealHerdr {
    /// With the app's environment.
    pub fn new(home: &Path) -> RealHerdr {
        RealHerdr::with_env(home, std::env::vars_os())
    }

    /// With `vars` as the environment, less the variables that name a thread or terminal.
    pub fn with_env(
        home: &Path,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> RealHerdr {
        RealHerdr {
            home: home.to_path_buf(),
            env: child_env(vars)
                .into_iter()
                .filter(|(k, _)| !HERDR_REMOVED_VARS.iter().any(|r| k == r))
                .collect(),
        }
    }

    /// Where `tool` is, or `None` if it isn't installed.
    pub fn program(&self, tool: Tool) -> Option<PathBuf> {
        let name = match tool {
            Tool::App => "herdr-app",
            Tool::Hc => "hc",
            Tool::Open => return Some(PathBuf::from("/usr/bin/open")),
        };
        let local = self.home.join(".local/bin").join(name);
        if local.is_file() {
            return Some(local);
        }
        let path = self.env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v)?;
        std::env::split_paths(path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    }
}

impl HerdrCli for RealHerdr {
    fn run(&mut self, tool: Tool, argv: &[String], timeout: Duration) -> io::Result<CmdOutput> {
        let program = self
            .program(tool)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        ProcessRunner::new(program, self.home.clone(), self.env.clone()).run(argv, None, timeout)
    }

    fn pause(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// The agent a tab runs. Codex needs `--no-daemon`, or its commands run outside hc. Herdr
/// passes the tab's `task` to it as one more argument: its first message.
pub fn agent_argv(agent: HerdrAgent) -> Vec<String> {
    match agent {
        HerdrAgent::Claude => vec!["claude".to_string()],
        HerdrAgent::Codex => vec!["codex".to_string(), "--no-daemon".to_string()],
    }
}

/// `herdr-app do tab.new {…}`: a tab in an existing space, without taking focus.
pub fn tab_new_argv(
    space: &str,
    name: &str,
    folder: &str,
    argv: &[String],
    task: &str,
) -> Vec<String> {
    let args = json!({
        "space": space,
        "name": name,
        "folder": folder,
        "argv": argv,
        "task": task,
        "focus": false,
    });
    vec!["do".to_string(), "tab.new".to_string(), args.to_string()]
}

/// `herdr-app do space.new {…}`: a space named `name` whose first tab is the thread's.
pub fn space_new_argv(
    name: &str,
    folder: &str,
    tab: &str,
    argv: &[String],
    task: &str,
) -> Vec<String> {
    let args = json!({
        "name": name,
        "folder": folder,
        "tab": tab,
        "argv": argv,
        "task": task,
        "focus": false,
    });
    vec!["do".to_string(), "space.new".to_string(), args.to_string()]
}

/// The labels every terminal Airboss starts carries, as `key=value`.
pub fn labels(task: TaskId, row: i64) -> Vec<String> {
    vec![
        "airboss=1".to_string(),
        format!("airboss.task={}", task.0),
        format!("airboss.launch={row}"),
    ]
}

/// `hc label <address> airboss=1 airboss.task=<id> airboss.launch=<row>`.
pub fn label_argv(address: &str, task: TaskId, row: i64) -> Vec<String> {
    let mut argv = vec!["label".to_string(), address.to_string()];
    argv.extend(labels(task, row));
    argv
}

/// `hc ls --json --label …`: the live terminals carrying a row's labels.
pub fn find_argv(task: TaskId, row: i64) -> Vec<String> {
    let mut argv = vec!["ls".to_string(), "--json".to_string()];
    for label in labels(task, row) {
        argv.push("--label".to_string());
        argv.push(label);
    }
    argv
}

/// `herdr-app ls`.
pub fn ls_argv() -> Vec<String> {
    vec!["ls".to_string()]
}

/// `open -g -b dev.herdr.app`: opens Herdr without bringing it forward, as tabs open
/// without taking focus.
pub fn open_argv() -> Vec<String> {
    ["-g", "-b", BUNDLE_ID].map(String::from).to_vec()
}

/// A terminal address Airboss will store and pass to `hc`: 1–64 printable ASCII characters
/// with no spaces (core's rule for `herdr_terminal`), starting with a letter or digit so it
/// can't be read as a flag.
pub fn valid_address(a: &str) -> bool {
    (1..=64).contains(&a.len())
        && a.bytes().all(|b| b.is_ascii_graphic())
        && a.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
}

/// One space from `herdr-app ls`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Space {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    /// `None` (or empty) for this computer.
    #[serde(default)]
    pub machine: Option<String>,
}

#[derive(Deserialize)]
struct Ls {
    spaces: Vec<Space>,
}

/// Reads `herdr-app ls`: `{spaces: [{id, name, folder, machine, tabs}], recent}`.
pub fn read_ls(stdout: &str) -> Option<Vec<Space>> {
    serde_json::from_str::<Ls>(stdout.trim())
        .ok()
        .map(|ls| ls.spaces)
}

/// The space a repo's tabs go in: on this computer, the one named `name`, else the one
/// whose folder is `folder`.
pub fn find_space<'a>(spaces: &'a [Space], name: &str, folder: &Path) -> Option<&'a Space> {
    let local: Vec<&Space> = spaces
        .iter()
        .filter(|s| s.machine.as_deref().is_none_or(str::is_empty))
        .collect();
    local
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .or_else(|| {
            local.iter().find(|s| {
                s.folder
                    .as_deref()
                    .is_some_and(|f| same_path(Path::new(f), folder))
            })
        })
        .copied()
}

/// Two paths name the same folder: equal, or equal once symlinks are resolved.
pub fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Where Herdr keeps worktrees: `worktrees.directory` in Herdr's `settings.json` (read
/// only), as an absolute path or `~`/`~/…`; else `~/.herdr/worktrees`.
pub fn worktrees_dir(settings: &Path, home: &Path) -> PathBuf {
    let default = home.join(".herdr/worktrees");
    let Ok(text) = std::fs::read_to_string(settings) else {
        return default;
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return default;
    };
    let Some(dir) = json
        .get("worktrees")
        .and_then(|w| w.get("directory"))
        .and_then(Value::as_str)
        .map(str::trim)
    else {
        return default;
    };
    let dir = if dir == "~" {
        home.to_path_buf()
    } else if let Some(rest) = dir.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(dir)
    };
    if dir.is_absolute() { dir } else { default }
}

/// `<dir>/<repo folder name>/<branch with / as ->`, as Herdr's own New worktree names it.
pub fn worktree_path(dir: &Path, repo_path: &Path, repo_name: &str, branch: &str) -> PathBuf {
    let folder = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo_name.replace('/', "-"));
    dir.join(folder).join(branch.replace('/', "-"))
}

/// `hc --version` (`hc 0.1.22`) and `herdr-app --version` (`herdr-app 0.1.18`): both
/// answering is `Ready` with herdr-app's version; anything else is `Missing`.
pub fn availability(cli: &mut dyn HerdrCli) -> Availability {
    let missing = || Availability::Missing {
        reason: errors::HERDR_MISSING.to_string(),
    };
    let version = ["--version".to_string()];
    let hc = read_version(cli.run(Tool::Hc, &version, QUICK_TIMEOUT), "hc");
    if hc.is_none() {
        return missing();
    }
    match read_version(cli.run(Tool::App, &version, QUICK_TIMEOUT), "herdr-app") {
        Some(version) => Availability::Ready { version },
        None => missing(),
    }
}

fn read_version(result: io::Result<CmdOutput>, name: &str) -> Option<String> {
    let out = result.ok().filter(|o| o.status == Some(0))?;
    let line = out.stdout.lines().next()?.trim();
    let v = line.strip_prefix(name)?.trim().trim_start_matches('v');
    let plain = !v.is_empty()
        && v.len() <= 40
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c));
    plain.then(|| v.to_string())
}

/// Whether the window answers `herdr-app status`.
fn window_answers(cli: &mut dyn HerdrCli) -> bool {
    matches!(
        cli.run(Tool::App, &["status".to_string()], STATUS_TIMEOUT),
        Ok(out) if out.status == Some(0)
    )
}

/// Opens Herdr and waits up to [`WINDOW_WAIT`] for its window. `true` once it answers.
fn open_window(cli: &mut dyn HerdrCli) -> bool {
    let opened = cli.run(Tool::Open, &open_argv(), QUICK_TIMEOUT);
    if !matches!(&opened, Ok(out) if out.status == Some(0)) {
        crate::log(&format!("open -b {BUNDLE_ID} didn't work: {opened:?}"));
        return false;
    }
    let began = Instant::now();
    let tries = WINDOW_WAIT.as_millis() / WINDOW_POLL.as_millis().max(1);
    for tried in 0..=tries {
        if window_answers(cli) {
            return true;
        }
        if tried == tries || began.elapsed() >= WINDOW_WAIT {
            break;
        }
        cli.pause(WINDOW_POLL);
    }
    false
}

/// What one `herdr-app` call came to, once the window was dealt with.
#[derive(Debug)]
pub enum AppRun {
    /// It ran with the window there; exit 0 or `herdr-app`'s own refusal.
    Ran(CmdOutput),
    /// It couldn't run, or never reached a window. Nothing was made.
    Failed(ServiceError),
    /// No answer in time: it may have made something.
    Unsure(ServiceError),
}

/// Runs `herdr-app <argv>`. If it fails and the window doesn't answer `status`, Herdr isn't
/// open: open it, wait up to 10 s for its window, and run the command once more. A command
/// that never reached a window made nothing, so running it again is safe.
pub fn run_app(cli: &mut dyn HerdrCli, argv: &[String], timeout: Duration) -> AppRun {
    let first = match cli.run(Tool::App, argv, timeout) {
        Ok(out) => out,
        Err(e) => return spawn_error(&e, timeout),
    };
    if first.status == Some(0) || window_answers(cli) {
        return AppRun::Ran(first);
    }
    if !open_window(cli) {
        return AppRun::Failed(errors::herdr_not_running());
    }
    match cli.run(Tool::App, argv, timeout) {
        Ok(out) if out.status == Some(0) || window_answers(cli) => AppRun::Ran(out),
        Ok(_) => AppRun::Failed(errors::herdr_not_running()),
        Err(e) => spawn_error(&e, timeout),
    }
}

fn spawn_error(e: &io::Error, timeout: Duration) -> AppRun {
    match e.kind() {
        io::ErrorKind::TimedOut => AppRun::Unsure(errors::herdr_timeout(timeout)),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            AppRun::Failed(errors::herdr_missing())
        }
        _ => AppRun::Failed(errors::herdr_failed(&format!("Couldn't run Herdr: {e}"))),
    }
}

/// `herdr-app`'s refusal: the `{"error": …}` it prints, else its first line, else the code.
pub fn refusal(out: &CmdOutput) -> ServiceError {
    let from_json = [&out.stdout, &out.stderr].into_iter().find_map(|text| {
        let v: Value = serde_json::from_str(text.trim()).ok()?;
        v.get("error")?.as_str().map(str::to_string)
    });
    let line = from_json.or_else(|| {
        [&out.stderr, &out.stdout]
            .into_iter()
            .flat_map(|t| t.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    });
    match (line, out.status) {
        (Some(line), _) => errors::herdr_failed(&line),
        (None, Some(code)) => {
            errors::herdr_failed(&format!("Herdr stopped with exit code {code}."))
        }
        (None, None) => errors::herdr_failed("Herdr was stopped."),
    }
}

/// Reads `herdr-app do tab.new` or `space.new`: exit 0 with `{"terminal": …}` (and the
/// space and tab ids) is a started terminal; a refusal made nothing; anything else may have.
pub fn read_opened(run: AppRun) -> Answer {
    let out = match run {
        AppRun::Ran(out) => out,
        AppRun::Failed(e) => return Answer::Failed(e),
        AppRun::Unsure(e) => return Answer::Unsure(e),
    };
    match out.status {
        Some(0) => {
            let terminal = serde_json::from_str::<Value>(out.stdout.trim())
                .ok()
                .and_then(|v| v.get("terminal")?.as_str().map(str::to_string))
                .filter(|t| valid_address(t));
            match terminal {
                Some(thread_id) => Answer::Started {
                    thread_id,
                    project_id: None,
                },
                None => Answer::Unsure(errors::herdr_bad_answer()),
            }
        }
        // A signal we didn't send: it may have got far enough to make the tab.
        None => Answer::Unsure(errors::herdr_bad_answer()),
        Some(_) => Answer::Failed(refusal(&out)),
    }
}

#[derive(Deserialize)]
struct HcLs {
    terminals: Vec<HcTerminal>,
}

#[derive(Deserialize)]
struct HcTerminal {
    address: String,
    #[serde(default)]
    state: Option<String>,
}

/// Reads `hc ls --json`: the first live terminal's address. `None` when it isn't JSON.
pub fn read_found(out: &CmdOutput) -> Option<Option<String>> {
    if out.status != Some(0) {
        return None;
    }
    let ls: HcLs = serde_json::from_str(out.stdout.trim()).ok()?;
    Some(
        ls.terminals
            .into_iter()
            .filter(|t| t.state.as_deref().is_none_or(|s| s == "live"))
            .map(|t| t.address)
            .find(|a| valid_address(a)),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn out(code: i32, stdout: &str, stderr: &str) -> CmdOutput {
        CmdOutput {
            status: Some(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn the_json_argument_holds_the_brief_as_it_is() {
        let task = "line 1\n\"quoted\" $HOME `x` --flag";
        let argv = tab_new_argv("sp1", "1 T", "/wt", &agent_argv(HerdrAgent::Claude), task);
        assert_eq!(argv.len(), 3);
        assert_eq!(argv[..2], ["do", "tab.new"]);
        let v: Value = serde_json::from_str(&argv[2]).unwrap();
        assert_eq!(v["task"], task);
        assert_eq!(v["argv"], json!(["claude"]));
        assert_eq!(v["focus"], false);
        assert_eq!(v["space"], "sp1");
        let argv = space_new_argv("web", "/wt", "1 T", &agent_argv(HerdrAgent::Codex), "x");
        let v: Value = serde_json::from_str(&argv[2]).unwrap();
        assert_eq!(v["argv"], json!(["codex", "--no-daemon"]));
        assert_eq!(v["tab"], "1 T");
        assert_eq!(v["name"], "web");
    }

    #[test]
    fn labels_and_lookups() {
        assert_eq!(
            label_argv("t4p", TaskId(7), 12),
            [
                "label",
                "t4p",
                "airboss=1",
                "airboss.task=7",
                "airboss.launch=12"
            ]
        );
        assert_eq!(
            find_argv(TaskId(7), 12),
            [
                "ls",
                "--json",
                "--label",
                "airboss=1",
                "--label",
                "airboss.task=7",
                "--label",
                "airboss.launch=12"
            ]
        );
    }

    #[test]
    fn addresses() {
        for ok in ["t4p", "vps/t7", "t1b"] {
            assert!(valid_address(ok), "{ok}");
        }
        for bad in ["", "-t1", "a b", "t\n", &"t".repeat(65), "tö"] {
            assert!(!valid_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn spaces_by_name_then_folder_on_this_computer() {
        let ls = r#"{"spaces":[
            {"id":"sp0","name":"web","folder":"/x","machine":"vps","tabs":[]},
            {"id":"sp1","name":"Personal","folder":"/src/web","machine":null,"tabs":[]},
            {"id":"sp2","name":"web","folder":"/y","machine":null,"tabs":[]},
            {"id":"sp3","name":null,"folder":"/src/api","tabs":[]}
        ],"recent":[]}"#;
        let spaces = read_ls(ls).unwrap();
        let id = |name: &str, folder: &str| {
            find_space(&spaces, name, Path::new(folder)).map(|s| s.id.clone())
        };
        assert_eq!(id("web", "/src/web").as_deref(), Some("sp2"));
        assert_eq!(id("booking", "/src/web").as_deref(), Some("sp1"));
        assert_eq!(id("api", "/src/api").as_deref(), Some("sp3"));
        assert_eq!(id("nope", "/nope"), None);
        assert!(read_ls("not json").is_none());
    }

    #[test]
    fn worktree_places() {
        let home = Path::new("/Users/me");
        let missing = Path::new("/nonexistent/settings.json");
        assert_eq!(
            worktrees_dir(missing, home),
            PathBuf::from("/Users/me/.herdr/worktrees")
        );
        assert_eq!(
            worktree_path(
                Path::new("/w"),
                Path::new("/src/booking-web"),
                "booking-web",
                "task-1/x"
            ),
            PathBuf::from("/w/booking-web/task-1-x")
        );
    }

    #[test]
    fn opened_answers() {
        let ok = AppRun::Ran(out(
            0,
            "{\n  \"space\": \"sp1\",\n  \"tab\": \"tb1\",\n  \"terminal\": \"t4z\"\n}\n",
            "",
        ));
        assert_eq!(
            read_opened(ok),
            Answer::Started {
                thread_id: "t4z".into(),
                project_id: None
            }
        );
        let refused = AppRun::Ran(out(1, "{\n  \"error\": \"no space \\\"sp9\\\"\"\n}\n", ""));
        match read_opened(refused) {
            Answer::Failed(e) => {
                assert_eq!(e.code, airboss_services::ErrorCode::HerdrFailed);
                assert_eq!(e.message, "no space \"sp9\"");
            }
            other => panic!("{other:?}"),
        }
        for odd in ["", "{}", "{\"terminal\":\"-x\"}", "{\"terminal\":\"a b\"}"] {
            assert!(
                matches!(read_opened(AppRun::Ran(out(0, odd, ""))), Answer::Unsure(_)),
                "{odd:?}"
            );
        }
    }

    #[test]
    fn found_terminals() {
        let o = out(
            0,
            r#"{"hidden_finished":0,"terminals":[{"address":"t4z","state":"live"}]}"#,
            "",
        );
        assert_eq!(read_found(&o), Some(Some("t4z".into())));
        let none = out(0, r#"{"terminals":[]}"#, "");
        assert_eq!(read_found(&none), Some(None));
        assert_eq!(read_found(&out(1, "", "boom")), None);
        assert_eq!(read_found(&out(0, "nope", "")), None);
    }
}
