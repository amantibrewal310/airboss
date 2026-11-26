//! Herdr terminals, from `hc events --json`.
//!
//! `hc events --json` prints one JSON line per fact. The first is a snapshot of every
//! terminal (`{channel, seq, terminals}`); after it come JSON-RPC notifications: `event`
//! (`params: {type, terminal, seq, data}`), `heartbeat` every 15 s, and `missed` with a
//! fresh snapshot for a reader that fell behind. A snapshot replaces what's known; each
//! event changes one terminal. Only the facts Decision 6 needs are kept: whether the
//! terminal is live, its program's status word, when that changed, its title and folder.
//!
//! Status comes from the program's own OSC 7501 reports, never from screen text.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_services::time::to_ts;
use airboss_services::{Source, ThreadKey, ThreadState, ThreadView};
use serde_json::Value;

use crate::worker::T3Msg;

/// One terminal, as far as `sync` follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrTerminal {
    /// `t4p`, or `vps/t7` on a connected machine: a `herdr_terminal` link's `ref`.
    pub address: String,
    /// `state: live`; `false` once `finished`.
    pub live: bool,
    /// The program's status word (`idle`, `working`, `done`, `blocked`, `error`), or `None`
    /// when it never reported one or has exited.
    pub status: Option<String>,
    /// When the status word last changed, in epoch milliseconds.
    pub status_ms: Option<u64>,
    /// Its title, else its name; `None` when it has neither.
    pub title: Option<String>,
    pub cwd: Option<String>,
}

/// One line of `hc events --json` that changes what `sync` knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrUpdate {
    /// Every terminal: the stream's first line, and again after `missed`.
    Snapshot {
        seq: u64,
        terminals: Vec<HerdrTerminal>,
    },
    /// A new terminal.
    Created { seq: u64, terminal: HerdrTerminal },
    /// Its program reported a status word, or its status cleared.
    Status {
        seq: u64,
        address: String,
        status: Option<String>,
    },
    Title {
        seq: u64,
        address: String,
        title: String,
    },
    /// Its program ended.
    Finished { seq: u64, address: String },
    /// It was removed.
    Removed { seq: u64, address: String },
}

impl HerdrUpdate {
    fn seq(&self) -> u64 {
        match self {
            HerdrUpdate::Snapshot { seq, .. }
            | HerdrUpdate::Created { seq, .. }
            | HerdrUpdate::Status { seq, .. }
            | HerdrUpdate::Title { seq, .. }
            | HerdrUpdate::Finished { seq, .. }
            | HerdrUpdate::Removed { seq, .. } => *seq,
        }
    }
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

/// A status object's word: `null` is none, `{state, …}` is its `state`.
fn status_word(v: Option<&Value>) -> Result<Option<String>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(s) => s
            .get("state")
            .and_then(Value::as_str)
            .map(|w| Some(w.to_string()))
            .ok_or_else(|| format!("a status without a state: {s}")),
    }
}

fn terminal(v: &Value) -> Result<HerdrTerminal, String> {
    let address = text(v, "address")
        .filter(|a| !a.is_empty())
        .ok_or("a terminal without an address")?;
    let live = match v.get("state").and_then(Value::as_str) {
        Some("live") => true,
        Some("finished") => false,
        other => return Err(format!("terminal {address} has state {other:?}")),
    };
    let title = text(v, "title")
        .filter(|t| !t.is_empty())
        .or_else(|| text(v, "name").filter(|n| !n.is_empty()));
    Ok(HerdrTerminal {
        status: status_word(v.get("status")).map_err(|e| format!("terminal {address}: {e}"))?,
        status_ms: v.get("status_ms").and_then(Value::as_u64),
        title,
        cwd: text(v, "cwd"),
        live,
        address,
    })
}

fn snapshot(v: &Value) -> Result<HerdrUpdate, String> {
    let list = v
        .get("terminals")
        .and_then(Value::as_array)
        .ok_or("a snapshot without terminals")?;
    Ok(HerdrUpdate::Snapshot {
        seq: v.get("seq").and_then(Value::as_u64).unwrap_or(0),
        terminals: list.iter().map(terminal).collect::<Result<_, _>>()?,
    })
}

/// Reads one line of `hc events --json`. `Ok(None)` for a line that changes nothing `sync`
/// follows (a heartbeat, a document, a terminal's size, foreground or labels); an error for
/// a line that isn't JSON, or a snapshot or event missing what it must have.
pub fn parse_line(line: &str) -> Result<Option<HerdrUpdate>, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let v: Value =
        serde_json::from_str(line).map_err(|e| format!("a line that isn't JSON ({e})"))?;
    if v.get("terminals").is_some() {
        return snapshot(&v).map(Some);
    }
    let params = v.get("params").unwrap_or(&Value::Null);
    match v.get("method").and_then(Value::as_str) {
        Some("event") => {}
        // A reader that fell behind gets a fresh snapshot, inside it or on the next line.
        Some("missed") if params.get("terminals").is_some() => return snapshot(params).map(Some),
        Some(_) => return Ok(None),
        None => return Err(format!("a line sync can't place: {line:.200}")),
    }
    let kind = params
        .get("type")
        .and_then(Value::as_str)
        .ok_or("an event without a type")?;
    let seq = params.get("seq").and_then(Value::as_u64).unwrap_or(0);
    let data = params.get("data").unwrap_or(&Value::Null);
    let address = || {
        text(params, "terminal")
            .filter(|a| !a.is_empty())
            .ok_or_else(|| format!("a {kind} event without a terminal"))
    };
    Ok(Some(match kind {
        "created" => HerdrUpdate::Created {
            seq,
            terminal: terminal(data)?,
        },
        "status" => HerdrUpdate::Status {
            seq,
            address: address()?,
            status: status_word(data.get("status"))?,
        },
        "title" => HerdrUpdate::Title {
            seq,
            address: address()?,
            title: text(data, "title").unwrap_or_default(),
        },
        "finished" => HerdrUpdate::Finished {
            seq,
            address: address()?,
        },
        "removed" => HerdrUpdate::Removed {
            seq,
            address: address()?,
        },
        // lost, restarted, keep, turn, foreground, size, labels, doc, and any new kind.
        _ => return Ok(None),
    }))
}

/// What's known about every terminal: the last snapshot, with each event since applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrTable {
    pub terminals: HashMap<String, HerdrTerminal>,
    /// The last snapshot's `seq`: events at or below it are already in it.
    seq: u64,
}

impl HerdrTable {
    /// Applies one update at `now`. Returns whether anything changed.
    pub fn apply(&mut self, u: HerdrUpdate, now: SystemTime) -> bool {
        if !matches!(u, HerdrUpdate::Snapshot { .. }) && u.seq() != 0 && u.seq() <= self.seq {
            return false;
        }
        let before = self.terminals.clone();
        match u {
            HerdrUpdate::Snapshot { seq, terminals } => {
                self.seq = seq;
                self.terminals = terminals
                    .into_iter()
                    .map(|t| (t.address.clone(), t))
                    .collect();
            }
            HerdrUpdate::Created { terminal, .. } => {
                self.terminals.insert(terminal.address.clone(), terminal);
            }
            HerdrUpdate::Status {
                address, status, ..
            } => {
                if let Some(t) = self.terminals.get_mut(&address) {
                    if t.status != status {
                        // The event doesn't carry `status_ms`; it's now, give or take the
                        // line's trip through the pipe.
                        t.status_ms = Some(epoch_ms(now));
                    }
                    t.status = status;
                }
            }
            HerdrUpdate::Title { address, title, .. } => {
                if let Some(t) = self.terminals.get_mut(&address)
                    && !title.is_empty()
                {
                    t.title = Some(title);
                }
            }
            HerdrUpdate::Finished { address, .. } => {
                if let Some(t) = self.terminals.get_mut(&address) {
                    t.live = false;
                }
            }
            HerdrUpdate::Removed { address, .. } => {
                self.terminals.remove(&address);
            }
        }
        self.terminals != before
    }
}

fn epoch_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Decision 6 of SPEC-herdr: `live` and `working` is running, `blocked` needs me, `error`
/// is errored, and `done`, `idle`, no status (or a word Herdr may add later) is idle. A
/// finished terminal, or one not listed, is gone.
pub fn state_of(t: Option<&HerdrTerminal>) -> ThreadState {
    match t {
        Some(t) if t.live => match t.status.as_deref() {
            Some("working") => ThreadState::Running,
            Some("blocked") => ThreadState::NeedsMe,
            Some("error") => ThreadState::Errored,
            _ => ThreadState::Idle,
        },
        _ => ThreadState::Gone,
    }
}

/// A view for each linked address. Herdr terminals are only ever reported when linked; a
/// blocked one needs me without saying what for (`request` is `None`).
pub fn views(table: &HerdrTable, linked: &[String]) -> Vec<ThreadView> {
    linked
        .iter()
        .map(|address| {
            let t = table.terminals.get(address);
            ThreadView {
                key: ThreadKey {
                    source: Source::Herdr,
                    r#ref: address.clone(),
                },
                linked: true,
                state: state_of(t),
                title: t.and_then(|t| t.title.clone()),
                request: None,
                error: None,
                run_id: None,
                project_path: t.and_then(|t| t.cwd.clone()),
                branch: None,
                worktree_path: None,
                parent_ref: None,
                child_count: 0,
                last_activity_at: t
                    .and_then(|t| t.status_ms)
                    .and_then(|ms| UNIX_EPOCH.checked_add(Duration::from_millis(ms)))
                    .map(to_ts),
                stale: false,
            }
        })
        .collect()
}

/// What one `hc events` stream sends the T3 worker, tagged with the stream's number so a
/// line from a stream already replaced is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrFeed {
    Update(HerdrUpdate),
    /// The stream stopped: `hc` exited, or sent a line `sync` can't read.
    Ended(String),
}

/// Where Herdr's terminals come from.
pub trait HerdrSource: Send {
    /// Starts following Herdr, stopping any stream already running. Each update goes to
    /// `tx` as [`T3Msg::Herdr`] with `stream`, and so does the stream's end. `NotFound` when
    /// `hc` isn't installed.
    fn follow(&mut self, stream: u64, tx: Sender<T3Msg>) -> io::Result<()>;
    /// Stops the stream, if one runs, and waits for its process.
    fn stop(&mut self);
}

/// `explicit` if it's a file, else the first `hc` on `path_env`.
pub fn find_hc(explicit: &Path, path_env: Option<&OsStr>) -> Option<PathBuf> {
    std::iter::once(explicit.to_path_buf())
        .chain(
            path_env
                .into_iter()
                .flat_map(std::env::split_paths)
                .map(|d| d.join("hc")),
        )
        .find(|p| p.is_file())
}

/// The real source: `hc events --json` as a child process, with a thread reading its lines.
/// Never used by tests, apart from the ones that hand it a script in place of `hc`.
pub struct RealHerdr {
    hc: PathBuf,
    child: Option<Child>,
}

impl RealHerdr {
    /// `hc` is `SyncConfig::hc_path` (`~/.local/bin/hc`); when it isn't there, `PATH` is
    /// searched on each start, so installing Herdr while the app runs is noticed.
    pub fn new(hc: &Path) -> RealHerdr {
        RealHerdr {
            hc: hc.to_path_buf(),
            child: None,
        }
    }

    /// The running child's pid, for tests.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }
}

impl HerdrSource for RealHerdr {
    fn follow(&mut self, stream: u64, tx: Sender<T3Msg>) -> io::Result<()> {
        self.stop();
        let path = std::env::var_os("PATH");
        let hc = find_hc(&self.hc, path.as_deref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "hc isn't installed"))?;
        let mut child = Command::new(hc)
            .args(["events", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let Some(out) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("hc's output couldn't be read"));
        };
        let reader = std::thread::Builder::new()
            .name("airboss-sync-herdr".into())
            .spawn(move || read_stream(BufReader::new(out), stream, &tx));
        if let Err(e) = reader {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Killing an exited child is harmless; waiting reaps it either way.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for RealHerdr {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Reads lines until the stream ends, sending each update. Lines that change nothing (the
/// 15 s heartbeat among them) never wake the worker.
fn read_stream(mut out: impl BufRead, stream: u64, tx: &Sender<T3Msg>) {
    let send = |feed| tx.send(T3Msg::Herdr { stream, feed }).is_ok();
    let mut line = String::new();
    let why = loop {
        line.clear();
        match out.read_line(&mut line) {
            Ok(0) => break "hc events stopped".to_string(),
            Ok(_) => match parse_line(&line) {
                Ok(Some(u)) => {
                    if !send(HerdrFeed::Update(u)) {
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => break format!("hc sent {e}"),
            },
            Err(e) => break format!("hc's output couldn't be read: {e}"),
        }
    };
    let _ = send(HerdrFeed::Ended(why));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn hc_is_found_at_its_place_else_on_path() {
        let dir = std::env::temp_dir().join(format!("airboss-find-hc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let hc = dir.join("hc");
        std::fs::write(&hc, "").unwrap();
        let missing = Path::new("/nonexistent/hc");
        assert_eq!(find_hc(&hc, None), Some(hc.clone()));
        assert_eq!(find_hc(missing, Some(dir.as_os_str())), Some(hc.clone()));
        assert_eq!(find_hc(missing, Some(OsStr::new("/nonexistent"))), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_status_event_without_status_ms_is_stamped_when_it_arrives() {
        let mut t = HerdrTable::default();
        let at = UNIX_EPOCH + Duration::from_secs(1_795_000_000);
        let snap = r#"{"channel":null,"seq":5,"terminals":[{"address":"t1","state":"live","status":null,"status_ms":null,"title":"","cwd":"/w"}]}"#;
        assert!(t.apply(parse_line(snap).unwrap().unwrap(), at));
        let ev = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"records":{},"reported":true,"status":{"state":"working"}},"seq":6,"terminal":"t1","type":"status"}}"#;
        assert!(t.apply(parse_line(ev).unwrap().unwrap(), at));
        assert_eq!(t.terminals["t1"].status_ms, Some(1_795_000_000_000));
        // The same word again changes nothing, not even the time.
        let again = ev.replace("\"seq\":6", "\"seq\":7");
        let later = at + Duration::from_secs(5);
        assert!(!t.apply(parse_line(&again).unwrap().unwrap(), later));
        assert_eq!(t.terminals["t1"].status_ms, Some(1_795_000_000_000));
    }
}
