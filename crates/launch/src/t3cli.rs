//! Everything about the `t3` CLI: the argv launch sends, and how it reads the answer.
//!
//! Facts come from T3's CLI source and `t3 … --help` (SPEC-launch, "Checked on this Mac").
//! Task text only ever goes in as one argv entry (`--title`) or on stdin (the brief).

use std::io;
use std::time::Duration;

use airboss_core::TaskId;
use airboss_services::{Availability, ErrorCode, ServiceError};
use serde::Deserialize;

use crate::brief::one_line;
use crate::errors;
use crate::process::{CmdOutput, ProcessRunner};

/// The longest thread title, in characters.
pub const TITLE_MAX: usize = 80;

/// Runs the `t3` CLI. The real one is a [`ProcessRunner`]; tests use `FakeT3Cli`.
pub trait T3Cli: Send {
    /// Runs `t3` with `argv`, writes `stdin` and closes it, and waits at most `timeout`.
    fn run(
        &mut self,
        argv: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> io::Result<CmdOutput>;
}

impl T3Cli for ProcessRunner {
    fn run(
        &mut self,
        argv: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        ProcessRunner::run(self, argv, stdin, timeout)
    }
}

/// `<id> <task title>`, plus ` · <repo>` when the task spans several repos, at most 80
/// characters. A cut shortens the task title and keeps the repo, which siblings go by.
pub fn thread_title(task: TaskId, title: &str, repo: Option<&str>) -> String {
    let head = format!("{} {}", task.0, one_line(title.trim()));
    let tail = repo
        .map(|r| format!(" · {}", one_line(r)))
        .unwrap_or_default();
    let room = TITLE_MAX.saturating_sub(tail.chars().count());
    if room < 8 {
        return cut(&(head + &tail), TITLE_MAX);
    }
    cut(&head, room) + &tail
}

/// `Plan <id> <task title>`, at most 80 characters.
pub fn plan_title(task: TaskId, title: &str) -> String {
    cut(
        &format!("Plan {} {}", task.0, one_line(title.trim())),
        TITLE_MAX,
    )
}

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Do it in one repo: a new worktree from `base` on `branch`, the brief on stdin.
pub fn do_it_argv(title: &str, base: &str, branch: &str, repo_path: &str) -> Vec<String> {
    [
        "thread",
        "start",
        "--json",
        "--title",
        title,
        "--worktree",
        base,
        "--branch",
        branch,
        repo_path,
        "-",
    ]
    .map(String::from)
    .to_vec()
}

/// Plan it, or research only: one thread in the folder itself, no worktree.
pub fn folder_argv(title: &str, folder: &str) -> Vec<String> {
    ["thread", "start", "--json", "--title", title, folder, "-"]
        .map(String::from)
        .to_vec()
}

/// "Add to T3": `project add --title <name> <path>`, flags first as its usage shows. A name
/// that could be read as a flag loses its leading `-`; with nothing left, T3 picks the title.
pub fn project_add_argv(name: &str, path: &str) -> Vec<String> {
    let title = one_line(name);
    let title = title.trim().trim_start_matches('-');
    let mut argv = vec!["project".to_string(), "add".to_string()];
    if !title.is_empty() {
        argv.push("--title".to_string());
        argv.push(title.to_string());
    }
    argv.push(path.to_string());
    argv
}

/// `--version`.
pub fn version_argv() -> Vec<String> {
    vec!["--version".to_string()]
}

/// What one `thread start` came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The thread exists.
    Started {
        thread_id: String,
        project_id: Option<String>,
    },
    /// No thread was made; the row becomes `failed`.
    Failed(ServiceError),
    /// A thread may exist; the row stays `pending`.
    Unsure(ServiceError),
}

impl Answer {
    /// The error, unless it started.
    pub fn error(&self) -> Option<&ServiceError> {
        match self {
            Answer::Started { .. } => None,
            Answer::Failed(e) | Answer::Unsure(e) => Some(e),
        }
    }
}

/// Whether T3 can't start anything after this error, so the rest of the places fail too.
pub fn stops_the_loop(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::T3Missing
            | ErrorCode::T3NotRunning
            | ErrorCode::T3Unreachable
            | ErrorCode::T3TooOld
            | ErrorCode::Timeout
    )
}

#[derive(Deserialize)]
struct StartJson {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "projectId", default)]
    project_id: Option<String>,
}

/// Reads `thread start --json`'s result. `place` names the repo or folder for messages.
pub fn read_start(result: io::Result<CmdOutput>, place: &str, timeout: Duration) -> Answer {
    let out = match result {
        Ok(out) => out,
        Err(e) => return spawn_error(&e, timeout),
    };
    match out.status {
        Some(0) => match parse_start(&out.stdout) {
            Some((thread_id, project_id)) => Answer::Started {
                thread_id,
                project_id,
            },
            None => Answer::Unsure(errors::bad_answer()),
        },
        // A signal we didn't send: it may have got far enough to make the thread.
        None => Answer::Unsure(errors::bad_answer()),
        Some(code) => Answer::Failed(read_failure(code, &out.stderr, place)),
    }
}

/// Reads any other command's result (`project add`): `Ok` on exit 0.
pub fn read_other(
    result: io::Result<CmdOutput>,
    place: &str,
    timeout: Duration,
) -> Result<CmdOutput, ServiceError> {
    match result {
        Ok(out) if out.status == Some(0) => Ok(out),
        Ok(out) => Err(match out.status {
            Some(code) => read_failure(code, &out.stderr, place),
            None => errors::bad_answer(),
        }),
        Err(e) => Err(match spawn_error(&e, timeout) {
            Answer::Failed(e) | Answer::Unsure(e) => e,
            Answer::Started { .. } => errors::bad_answer(),
        }),
    }
}

fn spawn_error(e: &io::Error, timeout: Duration) -> Answer {
    match e.kind() {
        io::ErrorKind::TimedOut => Answer::Unsure(errors::timeout(timeout)),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            Answer::Failed(errors::t3_missing())
        }
        // It never ran, so it made nothing.
        _ => Answer::Failed(errors::t3_failed(&format!("Couldn't run T3: {e}"))),
    }
}

/// The JSON line: `{"threadId": "…", "projectId": "…", "runId": …}`. The id must be plain
/// text, since it becomes a link ref and a deep link.
fn parse_start(stdout: &str) -> Option<(String, Option<String>)> {
    stdout.lines().rev().find_map(|line| {
        let json: StartJson = serde_json::from_str(line.trim()).ok()?;
        let plain = |s: &str| {
            !s.is_empty()
                && s.len() <= 200
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c))
        };
        plain(&json.thread_id).then(|| {
            let project = json.project_id.filter(|p| plain(p));
            (json.thread_id, project)
        })
    })
}

/// Maps `t3`'s one stderr line to its error, by how it starts (from T3's CLI source).
fn read_failure(code: i32, stderr: &str, place: &str) -> ServiceError {
    if code == 127 {
        return errors::t3_missing();
    }
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for line in &lines {
        if line.starts_with("No running T3 Code server uses") {
            return errors::t3_not_running();
        }
        if let Some(rest) = line.strip_prefix("Could not reach the T3 Code server") {
            let detail = rest.trim_start_matches([':', ' ', '.']);
            return errors::t3_unreachable(if detail.is_empty() { line } else { detail });
        }
        if line.starts_with("This T3 Code server has no") {
            return errors::t3_too_old();
        }
        if line.starts_with("No T3 Code project matches") {
            return errors::not_a_project(place);
        }
    }
    match lines.first() {
        Some(first) => errors::t3_failed(first),
        None => errors::t3_failed(&format!("T3 stopped with exit code {code}.")),
    }
}

/// `t3 --version`'s result: `t3 vX.Y.Z` with exit 0 is ready; anything else is missing.
pub fn read_version(result: io::Result<CmdOutput>) -> Availability {
    let missing = || Availability::Missing {
        reason: errors::T3_MISSING.to_string(),
    };
    let Ok(out) = result else {
        return missing();
    };
    if out.status != Some(0) {
        return missing();
    }
    let line = out.stdout.lines().next().unwrap_or("").trim();
    match line.strip_prefix("t3 ") {
        Some(v) => {
            let v = v.trim_start_matches('v');
            if !v.is_empty()
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
            {
                Availability::Ready {
                    version: v.to_string(),
                }
            } else {
                missing()
            }
        }
        None => missing(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const T: Duration = Duration::from_secs(30);

    fn exit(code: i32, stderr: &str) -> io::Result<CmdOutput> {
        Ok(CmdOutput {
            status: Some(code),
            stdout: String::new(),
            stderr: stderr.into(),
        })
    }

    fn stdout(text: &str) -> io::Result<CmdOutput> {
        Ok(CmdOutput {
            status: Some(0),
            stdout: text.into(),
            stderr: String::new(),
        })
    }

    fn failed(a: Answer) -> ServiceError {
        match a {
            Answer::Failed(e) => e,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    fn unsure(a: Answer) -> ServiceError {
        match a {
            Answer::Unsure(e) => e,
            other => panic!("expected Unsure, got {other:?}"),
        }
    }

    #[test]
    fn the_json_answer_starts_a_thread() {
        let a = read_start(
            stdout("{\"threadId\":\"97b5c8b5\",\"projectId\":\"p1\",\"runId\":null}\n"),
            "web",
            T,
        );
        assert_eq!(
            a,
            Answer::Started {
                thread_id: "97b5c8b5".into(),
                project_id: Some("p1".into())
            }
        );
        // A log line before the JSON is fine.
        let a = read_start(stdout("note: hi\n{\"threadId\":\"t-2\"}\n"), "web", T);
        assert!(matches!(a, Answer::Started { thread_id, project_id: None } if thread_id == "t-2"));
    }

    #[test]
    fn anything_else_on_stdout_leaves_it_pending() {
        for text in [
            "",
            "Started thread 97b5",
            "{\"threadId\":\"\"}",
            "{\"threadId\":\"a b\"}",
            "{\"threadId\":\"../x\"}",
            "{\"id\":\"x\"}",
            "[1,2]",
        ] {
            let e = unsure(read_start(stdout(text), "web", T));
            assert_eq!(e.code, ErrorCode::BadAnswer, "{text:?}");
        }
    }

    #[test]
    fn every_stderr_row_maps_to_its_code() {
        let rows = [
            (
                "No running T3 Code server uses /Users/me/.t3",
                ErrorCode::T3NotRunning,
            ),
            (
                "Could not reach the T3 Code server: ECONNREFUSED",
                ErrorCode::T3Unreachable,
            ),
            (
                "This T3 Code server has no t3_thread_launch tool",
                ErrorCode::T3TooOld,
            ),
            (
                "No T3 Code project matches /Users/me/src/web",
                ErrorCode::NotAProject,
            ),
            ("Something else went wrong", ErrorCode::T3Failed),
            ("", ErrorCode::T3Failed),
        ];
        for (line, code) in rows {
            let e = failed(read_start(exit(1, line), "booking-api", T));
            assert_eq!(e.code, code, "{line:?}");
        }
        let e = failed(read_start(
            exit(1, "Could not reach the T3 Code server: ECONNREFUSED"),
            "x",
            T,
        ));
        assert_eq!(e.message, "Couldn't reach T3 Code: ECONNREFUSED");
        let e = failed(read_start(
            exit(1, "No T3 Code project matches /x"),
            "booking-api",
            T,
        ));
        assert_eq!(e.message, "booking-api isn't a T3 project yet.");
        let e = failed(read_start(
            exit(1, "Something else went wrong\nat stack"),
            "x",
            T,
        ));
        assert_eq!(e.message, "Something else went wrong");
    }

    #[test]
    fn t3_missing_from_exit_127_or_no_binary() {
        let e = failed(read_start(
            exit(127, "T3 Code has moved or been removed."),
            "x",
            T,
        ));
        assert_eq!(e.code, ErrorCode::T3Missing);
        assert_eq!(e.message, errors::T3_MISSING);
        let gone = Err(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(failed(read_start(gone, "x", T)).code, ErrorCode::T3Missing);
    }

    #[test]
    fn a_timeout_or_a_kill_leaves_it_pending() {
        let e = unsure(read_start(
            Err(io::Error::from(io::ErrorKind::TimedOut)),
            "x",
            T,
        ));
        assert_eq!(e.code, ErrorCode::Timeout);
        assert_eq!(
            e.message,
            "T3 didn't answer in 30 s. The thread may still appear; Airboss will link it if it does."
        );
        let killed = Ok(CmdOutput {
            status: None,
            ..CmdOutput::default()
        });
        assert_eq!(
            unsure(read_start(killed, "x", T)).code,
            ErrorCode::BadAnswer
        );
    }

    #[test]
    fn which_errors_stop_the_loop() {
        for code in [
            ErrorCode::T3Missing,
            ErrorCode::T3NotRunning,
            ErrorCode::T3Unreachable,
            ErrorCode::T3TooOld,
            ErrorCode::Timeout,
        ] {
            assert!(stops_the_loop(code));
        }
        for code in [
            ErrorCode::NotAProject,
            ErrorCode::RepoMissing,
            ErrorCode::NoBaseBranch,
            ErrorCode::BadBranch,
            ErrorCode::BadAnswer,
            ErrorCode::T3Failed,
            ErrorCode::AlreadyStarting,
        ] {
            assert!(!stops_the_loop(code));
        }
    }

    #[test]
    fn versions() {
        assert_eq!(
            read_version(stdout("t3 v0.1.12\n")),
            Availability::Ready {
                version: "0.1.12".into()
            }
        );
        for bad in [
            stdout("hello"),
            stdout("t3 v"),
            stdout("t3 v1; rm"),
            exit(127, "gone"),
        ] {
            assert!(matches!(read_version(bad), Availability::Missing { .. }));
        }
        assert!(matches!(
            read_version(Err(io::Error::from(io::ErrorKind::NotFound))),
            Availability::Missing { reason } if reason == errors::T3_MISSING
        ));
    }

    #[test]
    fn titles() {
        assert_eq!(
            thread_title(TaskId(142), "Booking flow", None),
            "142 Booking flow"
        );
        assert_eq!(
            thread_title(TaskId(142), "Booking flow", Some("booking-web")),
            "142 Booking flow · booking-web"
        );
        let long = "word ".repeat(40);
        let t = thread_title(TaskId(142), &long, Some("booking-web"));
        assert!(t.chars().count() <= 80);
        assert!(t.ends_with("… · booking-web"), "{t}");
        let t = thread_title(TaskId(1), &long, None);
        assert!(t.chars().count() <= 80);
        assert!(t.ends_with('…'));
        assert_eq!(plan_title(TaskId(9), "Big\nthing"), "Plan 9 Big thing");
        assert!(plan_title(TaskId(9), &long).chars().count() <= 80);
        let exact = thread_title(TaskId(1), &"x".repeat(200), None);
        assert_eq!(exact.chars().count(), 80);
        // A very long repo name still gives 80.
        let t = thread_title(TaskId(1), "x", Some(&"r".repeat(100)));
        assert_eq!(t.chars().count(), 80);
    }

    #[test]
    fn argv_shapes() {
        assert_eq!(
            do_it_argv(
                "142 Booking flow",
                "main",
                "task-142-booking-flow",
                "/src/web"
            ),
            [
                "thread",
                "start",
                "--json",
                "--title",
                "142 Booking flow",
                "--worktree",
                "main",
                "--branch",
                "task-142-booking-flow",
                "/src/web",
                "-"
            ]
        );
        assert_eq!(
            folder_argv("Plan 1 x", "/src/web"),
            [
                "thread", "start", "--json", "--title", "Plan 1 x", "/src/web", "-"
            ]
        );
        assert_eq!(
            project_add_argv("web", "/src/web"),
            ["project", "add", "--title", "web", "/src/web"]
        );
        assert_eq!(
            project_add_argv("--force", "/src/web"),
            ["project", "add", "--title", "force", "/src/web"]
        );
        assert_eq!(
            project_add_argv("--", "/src/web"),
            ["project", "add", "/src/web"]
        );
    }
}
