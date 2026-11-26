//! Herdr, from the app's side: showing a terminal (Open thread) and reading the folders Herdr
//! knows (first run). Starting and following agents there are `launch`'s and `sync`'s. Only
//! `herdr-app ls`, `status` and `do agent.focus` run here, argv only, never through a shell.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Herdr.app's bundle id, for `open -b`.
pub const BUNDLE_ID: &str = "dev.herdr.app";
/// Open thread's toast when Herdr couldn't show the terminal.
pub const MSG_SHOW_FAILED: &str = "Couldn't show that terminal in Herdr.";
/// How long one `herdr-app` call may take. Both answer in milliseconds when the window is open.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// How long Open thread waits for a Herdr it just opened to answer `herdr-app status`.
pub const OPEN_WAIT: Duration = Duration::from_secs(5);
/// The pause between two `herdr-app status` while Herdr opens.
const STATUS_PAUSE: Duration = Duration::from_millis(250);

/// `herdr-app`: `~/.local/bin/herdr-app` (where Herdr installs it), then the app's `PATH`.
pub fn find_herdr_app(home: &Path, path: Option<&OsStr>) -> Option<PathBuf> {
    std::iter::once(home.join(".local/bin/herdr-app"))
        .chain(path.into_iter().flat_map(|p| {
            std::env::split_paths(p)
                .map(|d| d.join("herdr-app"))
                .collect::<Vec<_>>()
        }))
        .find(|p| runnable(p))
}

/// An executable file at `p`.
fn runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `herdr-app`'s arguments to show terminal `terminal`: `do agent.focus {"terminal":…}`, the
/// object as one argument, so the address is never read by a shell.
pub fn show_args(terminal: &str) -> Vec<String> {
    vec![
        "do".into(),
        "agent.focus".into(),
        serde_json::json!({ "terminal": terminal }).to_string(),
    ]
}

/// Runs `herdr-app` with `args`: its stdout, or why it failed (`herdr-app` prints
/// `{"error": …}` and exits 1 when a command fails).
pub fn run(home: &Path, args: &[String]) -> Result<String, String> {
    run_within(home, args, TIMEOUT)
}

fn run_within(home: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let app_path = std::env::var_os("PATH");
    let program = find_herdr_app(home, app_path.as_deref())
        .ok_or_else(|| "herdr-app isn't installed".to_string())?;
    let env: Vec<(OsString, OsString)> = airboss_launch::child_env(std::env::vars_os());
    let out = airboss_launch::ProcessRunner::new(program, home.to_path_buf(), env)
        .run(args, None, timeout)
        .map_err(|e| format!("couldn't run herdr-app: {e}"))?;
    if out.status == Some(0) {
        Ok(out.stdout)
    } else {
        let said = [&out.stdout, &out.stderr]
            .iter()
            .flat_map(|t| t.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("no output")
            .to_string();
        Err(format!("herdr-app exited with {:?}: {said}", out.status))
    }
}

/// What showing a terminal needs, so the order and the wait are tested without Herdr.
pub trait HerdrWindow {
    /// `open -b dev.herdr.app`: opens Herdr, or brings it forward.
    fn bring_forward(&mut self) -> Result<(), String>;
    /// `herdr-app <args>`.
    fn herdr_app(&mut self, args: &[String]) -> Result<String, String>;
    /// Waits `d` (between two `herdr-app status` while Herdr opens).
    fn pause(&mut self, d: Duration);
}

/// Open thread on terminal `terminal`: brings Herdr forward first, so a Herdr that was quit
/// opens, then `agent.focus`. If that fails because the window isn't up yet, waits up to
/// `wait` for `herdr-app status` to answer and tries once more.
pub fn show_terminal_in(
    w: &mut dyn HerdrWindow,
    terminal: &str,
    wait: Duration,
) -> Result<(), String> {
    w.bring_forward()?;
    let args = show_args(terminal);
    let first = match w.herdr_app(&args) {
        Ok(_) => return Ok(()),
        Err(e) => e,
    };
    let deadline = Instant::now() + wait;
    loop {
        if w.herdr_app(&["status".into()]).is_ok() {
            return w.herdr_app(&args).map(|_| ());
        }
        if Instant::now() >= deadline {
            return Err(format!("Herdr didn't answer after opening: {first}"));
        }
        w.pause(STATUS_PAUSE);
    }
}

/// The real Herdr: `/usr/bin/open` and `herdr-app`, argv only.
struct RealWindow<'a> {
    home: &'a Path,
}

impl HerdrWindow for RealWindow<'_> {
    fn bring_forward(&mut self) -> Result<(), String> {
        let out = airboss_launch::ProcessRunner::new(
            PathBuf::from("/usr/bin/open"),
            self.home.to_path_buf(),
            airboss_launch::child_env(std::env::vars_os()),
        )
        .run(&["-b".into(), BUNDLE_ID.into()], None, TIMEOUT)
        .map_err(|e| format!("couldn't bring Herdr forward: {e}"))?;
        match out.status {
            Some(0) => Ok(()),
            s => Err(format!("open -b {BUNDLE_ID} exited with {s:?}")),
        }
    }
    fn herdr_app(&mut self, args: &[String]) -> Result<String, String> {
        // `status` is asked again and again while Herdr opens: each try is short.
        let timeout = if args.first().map(String::as_str) == Some("status") {
            Duration::from_secs(1)
        } else {
            TIMEOUT
        };
        run_within(self.home, args, timeout)
    }
    fn pause(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Shows terminal `terminal` in Herdr (`show_terminal_in` with the real Herdr).
pub fn show_terminal(home: &Path, terminal: &str) -> Result<(), String> {
    show_terminal_in(&mut RealWindow { home }, terminal, OPEN_WAIT)
}

/// One folder Herdr knows: a space's, or one it opened recently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Deserialize)]
struct Ls {
    #[serde(default)]
    spaces: Vec<Named>,
    #[serde(default)]
    recent: Vec<Named>,
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: String,
    #[serde(default)]
    folder: Option<String>,
}

/// The folders in `herdr-app ls`'s JSON: each space's, then the recent ones, each once, by the
/// first name it had. `~` is the home folder; a folder that isn't absolute is left out.
pub fn folders_of(json: &str, home: &Path) -> Result<Vec<Folder>, String> {
    let ls: Ls = serde_json::from_str(json).map_err(|e| format!("herdr-app ls: {e}"))?;
    let mut out: Vec<Folder> = Vec::new();
    for n in ls.spaces.into_iter().chain(ls.recent) {
        let Some(folder) = n.folder.filter(|f| !f.is_empty()) else {
            continue;
        };
        let path = crate::classify::expand_home(&folder, home);
        if !path.is_absolute() || out.iter().any(|f| f.path == path) {
            continue;
        }
        let name = if n.name.trim().is_empty() {
            path.file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.clone())
        } else {
            n.name
        };
        out.push(Folder { name, path });
    }
    Ok(out)
}

/// The folders Herdr knows, for first run's Repos step.
pub trait HerdrFolders: Send + Sync {
    /// Its spaces' folders, then its recent ones; `Err` when Herdr can't be asked.
    fn folders(&self) -> Result<Vec<Folder>, String>;
}

/// Asks `herdr-app ls`.
pub struct RealHerdrFolders {
    pub home: PathBuf,
}

impl HerdrFolders for RealHerdrFolders {
    fn folders(&self) -> Result<Vec<Folder>, String> {
        let text = run(&self.home, &["ls".into()])?;
        folders_of(&text, &self.home)
    }
}

/// Answers what a test set: no Herdr at first.
#[derive(Debug)]
pub struct FakeHerdrFolders(Mutex<Result<Vec<Folder>, String>>);

impl Default for FakeHerdrFolders {
    fn default() -> FakeHerdrFolders {
        FakeHerdrFolders(Mutex::new(Err("herdr-app isn't installed".into())))
    }
}

impl FakeHerdrFolders {
    /// What `folders` answers from now on.
    pub fn answer(&self, folders: Result<Vec<Folder>, String>) {
        *lock(&self.0) = folders;
    }
}

impl HerdrFolders for FakeHerdrFolders {
    fn folders(&self) -> Result<Vec<Folder>, String> {
        lock(&self.0).clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_terminal_goes_as_one_json_argument() {
        assert_eq!(
            show_args("t4p"),
            vec!["do", "agent.focus", r#"{"terminal":"t4p"}"#]
        );
        // Whatever the ref holds stays inside the one JSON string.
        let odd = show_args("a\"; rm -rf ~ #");
        assert_eq!(odd.len(), 3);
        let v: serde_json::Value = serde_json::from_str(&odd[2]).unwrap();
        assert_eq!(v["terminal"], "a\"; rm -rf ~ #");
    }

    /// A Herdr whose window comes up once `status` has failed `status_fails` times
    /// (`agent.focus` fails until then), recording every call.
    struct Scripted {
        calls: Vec<String>,
        window_up: bool,
        status_fails: usize,
        open_fails: bool,
    }

    impl Scripted {
        fn new(window_up: bool, status_fails: usize) -> Scripted {
            Scripted {
                calls: Vec::new(),
                window_up,
                status_fails,
                open_fails: false,
            }
        }
    }

    impl HerdrWindow for Scripted {
        fn bring_forward(&mut self) -> Result<(), String> {
            self.calls.push("open -b dev.herdr.app".into());
            if self.open_fails {
                Err("open -b dev.herdr.app exited with Some(1)".into())
            } else {
                Ok(())
            }
        }
        fn herdr_app(&mut self, args: &[String]) -> Result<String, String> {
            self.calls.push(args.join(" "));
            match args[0].as_str() {
                "status" if self.status_fails > 0 => {
                    self.status_fails -= 1;
                    Err("no window".into())
                }
                "status" => {
                    self.window_up = true;
                    Ok("{}".into())
                }
                _ if self.window_up => Ok("{}".into()),
                _ => Err(r#"{"error":"the window isn't open"}"#.into()),
            }
        }
        fn pause(&mut self, _d: Duration) {
            self.calls.push("pause".into());
        }
    }

    const FOCUS: &str = r#"do agent.focus {"terminal":"t4p"}"#;

    #[test]
    fn herdr_comes_forward_first_then_shows_the_terminal() {
        let mut w = Scripted::new(true, 0);
        show_terminal_in(&mut w, "t4p", OPEN_WAIT).unwrap();
        assert_eq!(w.calls, vec!["open -b dev.herdr.app", FOCUS]);
    }

    #[test]
    fn a_herdr_that_was_quit_is_waited_for_then_asked_once_more() {
        let mut w = Scripted::new(false, 2);
        show_terminal_in(&mut w, "t4p", OPEN_WAIT).unwrap();
        assert_eq!(
            w.calls,
            vec![
                "open -b dev.herdr.app",
                FOCUS,
                "status",
                "pause",
                "status",
                "pause",
                "status",
                FOCUS
            ]
        );
    }

    #[test]
    fn a_herdr_that_never_answers_fails_after_the_wait_without_a_second_focus() {
        let mut w = Scripted::new(false, usize::MAX);
        let e = show_terminal_in(&mut w, "t4p", Duration::from_millis(30)).unwrap_err();
        assert!(e.contains("didn't answer"), "{e}");
        assert_eq!(w.calls.iter().filter(|c| *c == FOCUS).count(), 1);
        // Herdr that can't be opened at all: nothing else runs.
        let mut w = Scripted::new(true, 0);
        w.open_fails = true;
        assert!(show_terminal_in(&mut w, "t4p", OPEN_WAIT).is_err());
        assert_eq!(w.calls, vec!["open -b dev.herdr.app"]);
    }

    #[test]
    fn herdr_app_is_found_in_local_bin_first_then_on_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("airboss-herdr-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let other = dir.join("bin");
        for d in [home.join(".local/bin"), other.clone()] {
            std::fs::create_dir_all(&d).unwrap();
        }
        let path = std::env::join_paths([&other]).unwrap();
        assert_eq!(find_herdr_app(&home, Some(&path)), None);
        let on_path = other.join("herdr-app");
        std::fs::write(&on_path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_herdr_app(&home, Some(&path)), Some(on_path));
        let local = home.join(".local/bin/herdr-app");
        std::fs::write(&local, "#!/bin/sh\n").unwrap();
        // Not executable: skipped.
        assert_ne!(find_herdr_app(&home, Some(&path)), Some(local.clone()));
        std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_herdr_app(&home, Some(&path)), Some(local));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ls_gives_space_folders_then_recent_ones_each_once() {
        let json = r#"{"spaces":[
            {"id":"sp1","name":"booking","folder":"/w/booking","tabs":[{"id":"t1","name":"x","terminals":["t4p"]}]},
            {"id":"sp2","name":"Home","folder":"~/notes","tabs":[]},
            {"id":"sp3","name":"No folder","folder":null,"tabs":[]}
        ],"recent":[
            {"name":"booking again","folder":"/w/booking"},
            {"name":"","folder":"/w/api"},
            {"name":"relative","folder":"w/rel"}
        ]}"#;
        let got = folders_of(json, Path::new("/Users/me")).unwrap();
        let pairs: Vec<(&str, &Path)> = got
            .iter()
            .map(|f| (f.name.as_str(), f.path.as_path()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("booking", Path::new("/w/booking")),
                ("Home", Path::new("/Users/me/notes")),
                ("api", Path::new("/w/api")),
            ]
        );
        assert_eq!(folders_of("{}", Path::new("/")).unwrap(), vec![]);
        assert!(folders_of("not json", Path::new("/")).is_err());
    }
}
