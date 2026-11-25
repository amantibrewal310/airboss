//! Running a program with argv only: no shell, stdin written then closed, and a deadline.
//!
//! Used for `t3` and `git`. Task text only ever reaches a child as one argv entry or on
//! stdin, so nothing in it can be read as shell syntax.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Environment variables a child never inherits: they name the thread Airboss runs in (if
/// any), and a new thread must not think it is that thread.
pub const REMOVED_VARS: [&str; 3] = [
    "T3CODE_THREAD_ID",
    "CLAUDE_CODE_SESSION_ID",
    "AIRBOSS_THREAD",
];

/// The most of stdout or stderr kept; the rest is read and dropped so the child never blocks.
const OUTPUT_LIMIT: u64 = 1 << 20;

/// How long to wait for a child's output after it exits. A grandchild could hold the pipes.
const READ_GRACE: Duration = Duration::from_secs(1);

/// What a finished process printed, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CmdOutput {
    /// The exit code; `None` when a signal ended it.
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// The environment a child gets: `vars` without [`REMOVED_VARS`].
pub fn child_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(k, _)| !REMOVED_VARS.iter().any(|r| k == r))
        .collect()
}

/// Runs one program, from one directory, with exactly the environment it was given.
#[derive(Debug, Clone)]
pub struct ProcessRunner {
    program: PathBuf,
    cwd: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl ProcessRunner {
    /// `env` is the child's whole environment; build it with [`child_env`].
    pub fn new(program: PathBuf, cwd: PathBuf, env: Vec<(OsString, OsString)>) -> ProcessRunner {
        ProcessRunner { program, cwd, env }
    }

    /// The program this runs.
    pub fn program(&self) -> &PathBuf {
        &self.program
    }

    /// Runs the program with `argv`, writes `stdin` (if any) and closes it, and waits at most
    /// `timeout`. A missing program is `ErrorKind::NotFound`; running out of time kills the
    /// child and is `ErrorKind::TimedOut`.
    pub fn run(
        &self,
        argv: &[String],
        stdin: Option<&str>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        let deadline = Instant::now() + timeout;
        let mut child = Command::new(&self.program)
            .args(argv)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let (Some(mut pipe), Some(text)) = (child.stdin.take(), stdin) {
            let text = text.to_owned();
            // Its own thread, so a child that writes before reading can't deadlock us. The
            // pipe closes when the thread ends.
            thread::spawn(move || {
                let _ = pipe.write_all(text.as_bytes());
            });
        }
        let stdout = read_in_background(child.stdout.take());
        let stderr = read_in_background(child.stderr.take());
        let status = wait_until(&mut child, deadline)?;
        Ok(CmdOutput {
            status: status.code(),
            stdout: stdout.recv_timeout(READ_GRACE).unwrap_or_default(),
            stderr: stderr.recv_timeout(READ_GRACE).unwrap_or_default(),
        })
    }
}

/// Waits for the child, killing it at `deadline`.
fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<std::process::ExitStatus> {
    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the process didn't finish in time and was stopped",
            ));
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(Duration::from_millis(20));
    }
}

/// Reads a pipe to its end on its own thread; the text arrives on the channel.
fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            thread::spawn(move || {
                let mut kept = Vec::new();
                let _ = (&mut pipe).take(OUTPUT_LIMIT).read_to_end(&mut kept);
                let _ = io::copy(&mut pipe, &mut io::sink());
                let _ = tx.send(String::from_utf8_lossy(&kept).into_owned());
            });
        }
        None => {
            let _ = tx.send(String::new());
        }
    }
    rx
}
