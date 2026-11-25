//! Helpers shared by the integration tests. Each test file uses only some of them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-mcp-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    pub fn db(&self) -> PathBuf {
        self.0.join("airboss.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The `airboss` binary, with a clean environment: only `AIRBOSS_DB` pointing at `db`, and
/// none of the thread variables this test process may have inherited.
pub fn airboss(db: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_airboss"));
    for var in [
        "T3CODE_THREAD_ID",
        "CLAUDE_CODE_SESSION_ID",
        "AIRBOSS_THREAD",
        "NO_COLOR",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("AIRBOSS_DB", db);
    cmd
}

/// Runs the CLI with `args` and returns what it did.
pub fn run(db: &Path, args: &[&str]) -> Output {
    airboss(db).args(args).output().unwrap()
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

/// A real `airboss mcp` process, spoken to over its stdin and stdout.
pub struct Mcp {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    /// Starts `airboss mcp` on `db` as T3 thread `thread` (none: no thread variables).
    pub fn start(db: &Path, thread: Option<&str>) -> Mcp {
        let mut cmd = airboss(db);
        if let Some(t) = thread {
            cmd.env("T3CODE_THREAD_ID", t);
        }
        Mcp::spawn(cmd.arg("mcp"))
    }

    pub fn spawn(cmd: &mut Command) -> Mcp {
        use std::process::Stdio;
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        Mcp {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn send(&mut self, message: &serde_json::Value) {
        use std::io::Write;
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Reads one line of stdout; every line must be a JSON-RPC message.
    pub fn read(&mut self) -> serde_json::Value {
        use std::io::BufRead;
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "airboss mcp closed stdout");
        let msg: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("not a JSON-RPC message ({e}): {line}"));
        assert_eq!(msg["jsonrpc"], "2.0", "{line}");
        msg
    }

    /// Sends a request and returns its response, skipping any notifications.
    pub fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        );
        loop {
            let msg = self.read();
            if msg["id"] == id {
                return msg;
            }
        }
    }

    /// The `initialize` handshake; returns its result.
    pub fn initialize(&mut self) -> serde_json::Value {
        let resp = self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "airboss-tests", "version": "1"}
            }),
        );
        self.send(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        resp["result"].clone()
    }

    /// Calls a tool; returns (is_error, text).
    pub fn call(&mut self, tool: &str, args: serde_json::Value) -> (bool, String) {
        let resp = self.request(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        );
        let result = &resp["result"];
        assert!(result.is_object(), "not a tool result: {resp}");
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        (result["isError"] == true, text)
    }

    /// Calls a tool that must succeed, and parses its JSON.
    pub fn ok(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let (is_error, text) = self.call(tool, args);
        assert!(!is_error, "{tool} failed: {text}");
        serde_json::from_str(&text).unwrap()
    }

    /// Closes stdin and waits for the process to exit; returns whether it exited cleanly.
    pub fn finish(mut self) -> bool {
        drop(self.stdin);
        self.child.wait().unwrap().success()
    }
}
