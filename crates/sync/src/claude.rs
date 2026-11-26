//! Claude Code sessions run outside T3, from `~/.claude/sessions/<pid>.json`.
//!
//! Only files named by digits and `.json` are opened; the `.key` files beside them never
//! are. A session is alive when `ps` reports its pid with a start time equal to its
//! `procStart`, so a reused pid isn't mistaken for it.
//!
//! Claude Code writes a new session's file before all its fields: a file missing one is
//! skipped while it's fresh, and breaks Claude sync once it's older than [`FRESH`].

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_services::time::to_ts;
use airboss_services::{Source, ThreadKey, ThreadState, ThreadView};
use serde_json::Value;

/// One `<pid>.json` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSession {
    pub file: PathBuf,
    pub pid: u32,
    /// A `claude_session` link's `ref`.
    pub session_id: String,
    pub cwd: Option<String>,
    pub entrypoint: Option<String>,
    pub name: Option<String>,
    pub status: String,
    /// When the process started, as `ps -o lstart=` prints it in UTC.
    pub proc_start: String,
    /// Epoch milliseconds.
    pub updated_at: Option<i64>,
    pub status_updated_at: Option<i64>,
}

/// One read of the sessions folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFiles {
    /// The files read, sorted by file name.
    pub sessions: Vec<ClaudeSession>,
    /// Files caught mid-write and skipped this round. They stay watched, so their next
    /// write reads them again.
    pub skipped: Vec<PathBuf>,
}

/// How long a session file may lack a required field before that breaks Claude sync. A
/// new session's file lacks its `status` for about a second and a half.
pub const FRESH: Duration = Duration::from_secs(10);

/// Where Claude sessions come from.
pub trait ClaudeSource: Send {
    /// Every session file. `NotFound` when the folder doesn't exist; `InvalidData` naming
    /// the file and field when one older than [`FRESH`] lacks a required field.
    fn sessions(&mut self) -> io::Result<SessionFiles>;
    /// Whether each session's process is still the one that wrote the file.
    fn alive(&mut self, sessions: &[ClaudeSession]) -> io::Result<Vec<bool>>;
}

/// The entrypoint of a session T3 itself runs. Those are T3 threads: reported only when
/// linked as a `claude_session`, never as untracked.
pub const T3_ENTRYPOINT: &str = "t3-code";

/// Whether a file in the sessions folder is a session file: `<digits>.json`.
pub fn is_session_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "json")
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

/// Reads every session file in `dir`, sorted by file name. A file caught mid-write is
/// skipped: one that isn't valid JSON, or one modified less than [`FRESH`] ago that lacks a
/// required field. The next event reads it again.
pub fn read_sessions(dir: &Path) -> io::Result<SessionFiles> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_session_file(p))
        .collect();
    files.sort();
    let mut out = SessionFiles::default();
    for file in files {
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            // Gone between listing and reading: the session just ended.
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            out.skipped.push(file);
            continue;
        };
        match parse_session(&file, &value) {
            Ok(session) => out.sessions.push(session),
            Err(_) if is_fresh(&file) => out.skipped.push(file),
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Modified less than [`FRESH`] ago, or gone. A future mtime counts as fresh.
fn is_fresh(file: &Path) -> bool {
    match std::fs::metadata(file).and_then(|m| m.modified()) {
        Ok(at) => SystemTime::now()
            .duration_since(at)
            .map_or(true, |age| age < FRESH),
        Err(e) => e.kind() == io::ErrorKind::NotFound,
    }
}

fn parse_session(file: &Path, v: &Value) -> io::Result<ClaudeSession> {
    let missing = |field: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Claude session file {} has no {field}", file.display()),
        )
    };
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let pid = v
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|p| u32::try_from(p).ok())
        .ok_or_else(|| missing("pid"))?;
    Ok(ClaudeSession {
        file: file.to_path_buf(),
        pid,
        session_id: text("sessionId")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| missing("sessionId"))?,
        cwd: text("cwd"),
        entrypoint: text("entrypoint"),
        name: text("name"),
        status: text("status").ok_or_else(|| missing("status"))?,
        proc_start: text("procStart").ok_or_else(|| missing("procStart"))?,
        updated_at: v.get("updatedAt").and_then(Value::as_i64),
        status_updated_at: v.get("statusUpdatedAt").and_then(Value::as_i64),
    })
}

/// A session's state from its `status`:
///
/// - `busy` is running, and so is `shell`: the turn has ended but a background shell task
///   still runs, which Claude Code's own session list counts as busy.
/// - `idle` is idle.
/// - `waiting` (on me: a question or a permission, named by `waitingFor`) needs me.
///
/// Claude Code adds statuses over time. Any other is idle: the session is still shown, and
/// neither it nor Claude sync breaks. [`unknown_statuses`] names those for the log.
pub fn state_of(status: &str) -> ThreadState {
    match status {
        "busy" | "shell" => ThreadState::Running,
        "waiting" => ThreadState::NeedsMe,
        _ => ThreadState::Idle,
    }
}

const KNOWN_STATUSES: [&str; 4] = ["busy", "shell", "idle", "waiting"];

/// The statuses these sessions use that [`state_of`] doesn't know, sorted, each once.
pub fn unknown_statuses(sessions: &[ClaudeSession]) -> Vec<&str> {
    let mut out: Vec<&str> = sessions
        .iter()
        .map(|s| s.status.as_str())
        .filter(|s| !KNOWN_STATUSES.contains(s))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// `ps`'s arguments for these pids.
pub fn ps_args(pids: &[u32]) -> Vec<String> {
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    vec![
        "-p".into(),
        list.join(","),
        "-o".into(),
        "pid=,lstart=".into(),
    ]
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Matches `ps -o pid=,lstart=` output (in UTC) against each session's `procStart`.
pub fn parse_ps(output: &str, sessions: &[ClaudeSession]) -> Vec<bool> {
    let started: HashMap<u32, String> = output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (pid, rest) = line.split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, squash(rest)))
        })
        .collect();
    sessions
        .iter()
        .map(|s| started.get(&s.pid) == Some(&squash(&s.proc_start)))
        .collect()
}

fn millis(ms: i64) -> Option<SystemTime> {
    let ms = u64::try_from(ms).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_millis(ms))
}

fn view(s: &ClaudeSession, linked: bool, state: ThreadState) -> ThreadView {
    let last = [s.updated_at, s.status_updated_at]
        .into_iter()
        .flatten()
        .max()
        .and_then(millis)
        .map(to_ts);
    ThreadView {
        key: ThreadKey {
            source: Source::Claude,
            r#ref: s.session_id.clone(),
        },
        linked,
        state,
        title: s.name.clone(),
        request: None,
        error: None,
        run_id: None,
        project_path: s.cwd.clone(),
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: last,
        last_reply: None,
        stale: false,
    }
}

/// A linked session with no file: it ended.
fn missing(id: &str) -> ThreadView {
    ThreadView {
        key: ThreadKey {
            source: Source::Claude,
            r#ref: id.to_string(),
        },
        linked: true,
        state: ThreadState::Gone,
        title: None,
        request: None,
        error: None,
        run_id: None,
        project_path: None,
        branch: None,
        worktree_path: None,
        parent_ref: None,
        child_count: 0,
        last_activity_at: None,
        last_reply: None,
        stale: false,
    }
}

/// The views for one Claude pass.
///
/// - `open_linked`: the session ids linked to open tasks; each gets a view, `gone` when its
///   file is missing or its process is dead.
/// - `linked_anywhere`: every `claude_session` link's ref; the rest of the live sessions are
///   untracked, except those T3 runs.
///
pub fn views(
    sessions: &[ClaudeSession],
    alive: &[bool],
    open_linked: &[String],
    linked_anywhere: &HashSet<String>,
) -> Vec<ThreadView> {
    // A session id written by two files (one stale): the newest wins.
    let mut by_id: HashMap<&str, (&ClaudeSession, bool)> = HashMap::new();
    for (s, &up) in sessions.iter().zip(alive) {
        let newer = by_id
            .get(s.session_id.as_str())
            .is_none_or(|(old, old_up)| (up, s.updated_at) > (*old_up, old.updated_at));
        if newer {
            by_id.insert(&s.session_id, (s, up));
        }
    }
    let mut out = Vec::new();
    for id in open_linked {
        match by_id.get(id.as_str()) {
            Some((s, true)) => out.push(view(s, true, state_of(&s.status))),
            Some((s, false)) => out.push(view(s, true, ThreadState::Gone)),
            None => out.push(missing(id)),
        }
    }
    let mut untracked: Vec<&ClaudeSession> = by_id
        .values()
        .filter(|(s, up)| {
            *up && !linked_anywhere.contains(&s.session_id)
                && s.entrypoint.as_deref() != Some(T3_ENTRYPOINT)
        })
        .map(|(s, _)| *s)
        .collect();
    untracked.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    for s in untracked {
        out.push(view(s, false, state_of(&s.status)));
    }
    out
}

/// The real source: the session folder, and `/bin/ps` for liveness. Never used by tests.
pub struct RealClaude {
    dir: PathBuf,
}

impl RealClaude {
    pub fn new(dir: &Path) -> RealClaude {
        RealClaude {
            dir: dir.to_path_buf(),
        }
    }
}

impl ClaudeSource for RealClaude {
    fn sessions(&mut self) -> io::Result<SessionFiles> {
        read_sessions(&self.dir)
    }

    fn alive(&mut self, sessions: &[ClaudeSession]) -> io::Result<Vec<bool>> {
        if sessions.is_empty() {
            return Ok(Vec::new());
        }
        let pids: Vec<u32> = sessions.iter().map(|s| s.pid).collect();
        // `ps` exits 1 when some pid is gone, and still prints the others.
        let out = Command::new("/bin/ps")
            .args(ps_args(&pids))
            .env("TZ", "UTC")
            .env("LC_ALL", "C")
            .output()?;
        Ok(parse_ps(&String::from_utf8_lossy(&out.stdout), sessions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_digit_json_files_are_sessions() {
        assert!(is_session_file(Path::new("/s/3080.json")));
        assert!(!is_session_file(Path::new("/s/3080.69fb.key")));
        assert!(!is_session_file(Path::new("/s/abc.json")));
        assert!(!is_session_file(Path::new("/s/.json")));
        assert!(!is_session_file(Path::new("/s/3080.json.tmp")));
    }

    #[test]
    fn ps_arguments() {
        assert_eq!(ps_args(&[1, 22]), vec!["-p", "1,22", "-o", "pid=,lstart="]);
    }
}
