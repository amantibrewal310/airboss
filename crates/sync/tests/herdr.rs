//! Herdr: reading `hc events --json` lines (as captured from `hc` 0.1.22 on 26 Nov 2026),
//! Decision 6's mapping, and the real child-process source run against a script standing in
//! for `hc`, so no real terminal is touched.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use airboss_services::{RequestKind, Source, ThreadKey, ThreadState};
use airboss_sync::T3Msg;
use airboss_sync::fake::TempDir;
use airboss_sync::herdr::{
    self, HerdrFeed, HerdrSource, HerdrTable, HerdrTerminal, HerdrUpdate, RealHerdr, parse_line,
};

// Lines as `hc events --json` printed them (trimmed of fields sync doesn't read).
const SNAPSHOT: &str = r#"{"channel":null,"seq":1227,"terminals":[{"address":"t4n","argv":["zsh"],"cols":80,"created_ms":1795589000000,"cwd":"/Users/me","exit_code":0,"finished_ms":1795589736442,"labels":{},"name":null,"pid":null,"reason":"exited","rows":24,"screen":"exact","state":"finished","status":null,"status_ms":null,"title":"","turn":0},{"address":"t4p","argv":["claude"],"cwd":"/Users/me/work","labels":{"herdr.space":"sp1"},"name":null,"pid":4242,"state":"live","status":{"app":"claude-code","state":"done"},"status_ms":1795592904598,"title":"Fix the login bug","turn":3}]}"#;
const CREATED: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"address":"t4x","argv":["/tmp/probe.sh"],"cols":200,"created_ms":1795671894622,"cwd":"/tmp","exit_code":null,"finished_ms":null,"labels":{"airboss-probe":"1"},"name":"airboss-probe-a","pid":88085,"state":"live","status":null,"status_ms":null,"title":"","turn":0},"seq":1228,"terminal":"t4x","type":"created"}}"#;
const FOREGROUND: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"argv":["/bin/sh","/tmp/probe.sh"],"cwd":"/tmp","name":"bash","pid":88085},"seq":1229,"terminal":"t4x","type":"foreground"}}"#;
const WORKING: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"records":{},"reported":true,"status":{"state":"working"}},"seq":1230,"terminal":"t4x","type":"status"}}"#;
const BLOCKED: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"records":{},"reported":true,"status":{"app":"probe","state":"blocked"}},"seq":1231,"terminal":"t4x","type":"status"}}"#;
const TURN: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"state":"blocked","turn":1},"seq":1232,"terminal":"t4x","type":"turn"}}"#;
const TITLE: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"title":"probe title"},"seq":1234,"terminal":"t4x","type":"title"}}"#;
const CLEARED: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"records":{},"reported":true,"status":null},"seq":1235,"terminal":"t4x","type":"status"}}"#;
const FINISHED: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"exit_code":0,"finished_ms":1795671907045,"reason":"exited","signal":null},"seq":1236,"terminal":"t4x","type":"finished"}}"#;
const HEARTBEAT: &str = r#"{"jsonrpc":"2.0","method":"heartbeat","params":{"seq":1236}}"#;
const DOC: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"name":"herdr-app","version":152},"seq":1238,"type":"doc"}}"#;
const LABELS: &str = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"labels":{"where":"x / y"}},"seq":1241,"terminal":"t4y","type":"labels"}}"#;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_795_672_000)
}

fn update(line: &str) -> HerdrUpdate {
    parse_line(line).unwrap().unwrap()
}

#[test]
fn a_snapshot_line_lists_every_terminal() {
    let HerdrUpdate::Snapshot { seq, terminals } = update(SNAPSHOT) else {
        panic!("not a snapshot");
    };
    assert_eq!(seq, 1227);
    assert_eq!(
        terminals,
        vec![
            HerdrTerminal {
                address: "t4n".into(),
                live: false,
                status: None,
                status_kind: None,
                status_msg: None,
                status_ms: None,
                title: None,
                cwd: Some("/Users/me".into()),
            },
            HerdrTerminal {
                address: "t4p".into(),
                live: true,
                status: Some("done".into()),
                status_kind: None,
                status_msg: None,
                status_ms: Some(1_795_592_904_598),
                title: Some("Fix the login bug".into()),
                cwd: Some("/Users/me/work".into()),
            },
        ]
    );
}

#[test]
fn event_lines_become_changes_and_the_rest_are_dropped() {
    assert!(
        matches!(update(CREATED), HerdrUpdate::Created { seq: 1228, ref terminal }
        if terminal.address == "t4x" && terminal.title.as_deref() == Some("airboss-probe-a"))
    );
    assert_eq!(
        update(BLOCKED),
        HerdrUpdate::Status {
            seq: 1231,
            address: "t4x".into(),
            status: Some("blocked".into()),
            kind: None,
            msg: None,
        }
    );
    assert_eq!(
        update(CLEARED),
        HerdrUpdate::Status {
            seq: 1235,
            address: "t4x".into(),
            status: None,
            kind: None,
            msg: None,
        }
    );
    assert_eq!(
        update(TITLE),
        HerdrUpdate::Title {
            seq: 1234,
            address: "t4x".into(),
            title: "probe title".into()
        }
    );
    assert_eq!(
        update(FINISHED),
        HerdrUpdate::Finished {
            seq: 1236,
            address: "t4x".into()
        }
    );
    let removed = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{},"seq":9,"terminal":"t4x","type":"removed"}}"#;
    assert_eq!(
        update(removed),
        HerdrUpdate::Removed {
            seq: 9,
            address: "t4x".into()
        }
    );
    for quiet in [FOREGROUND, TURN, HEARTBEAT, DOC, LABELS, "", "  \n"] {
        assert_eq!(parse_line(quiet), Ok(None), "{quiet}");
    }
    let missed_with_snapshot = format!(
        r#"{{"jsonrpc":"2.0","method":"missed","params":{}}}"#,
        SNAPSHOT
    );
    assert!(matches!(
        update(&missed_with_snapshot),
        HerdrUpdate::Snapshot { seq: 1227, .. }
    ));
}

#[test]
fn only_lines_that_arent_snapshots_or_notifications_are_errors() {
    for bad in [
        "not json",
        r#"{"seq":1,"terminals":{}}"#,
        r#"{"something":"else"}"#,
    ] {
        assert!(parse_line(bad).is_err(), "{bad}");
    }
    // Events without what they name are skipped.
    for skipped in [
        r#"{"jsonrpc":"2.0","method":"event","params":{"data":{},"seq":2,"type":"status"}}"#,
        r#"{"jsonrpc":"2.0","method":"event","params":{"data":{}}}"#,
        r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"state":"live"},"seq":2,"type":"created"}}"#,
    ] {
        assert_eq!(parse_line(skipped), Ok(None), "{skipped}");
    }
}

/// One terminal Herdr reports oddly (a remote one in a state sync doesn't know, a status
/// with no word, fields missing) never fails its line, so it can't break Herdr for good.
#[test]
fn an_odd_terminal_is_read_tolerantly_and_never_fails_its_line() {
    let line = r#"{"seq":1,"terminals":[
        {"state":"live"},
        {"address":"vps/t7","state":"lost","status":{"app":"x"}},
        {"address":"t2","status":"working"},
        {"address":"t3","state":"live","status":{"state":"working"},"status_ms":"soon","title":7},
        {"address":"t4p","state":"live","status":{"state":"blocked"}}]}"#
        .replace('\n', "");
    let HerdrUpdate::Snapshot { terminals, .. } = update(&line) else {
        panic!("not a snapshot");
    };
    let by: std::collections::HashMap<&str, &HerdrTerminal> =
        terminals.iter().map(|t| (t.address.as_str(), t)).collect();
    assert_eq!(
        terminals.len(),
        4,
        "only the one without an address is skipped"
    );
    assert!(!by["vps/t7"].live, "an unknown state isn't live");
    assert_eq!(by["vps/t7"].status, None, "a status without a word is none");
    assert!(!by["t2"].live && by["t2"].status.is_none());
    assert_eq!(by["t3"].status.as_deref(), Some("working"));
    assert_eq!((by["t3"].status_ms, &by["t3"].title), (None, &None));
    let mut t = HerdrTable::default();
    t.apply(update(&line), now());
    assert_eq!(
        herdr::state_of(t.terminals.get("vps/t7")),
        ThreadState::Gone
    );
    assert_eq!(herdr::state_of(t.terminals.get("t3")), ThreadState::Running);
    assert_eq!(
        herdr::state_of(t.terminals.get("t4p")),
        ThreadState::NeedsMe
    );
    // An odd created event is read the same way.
    let created = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"state":"pending","status":[]},"seq":2,"terminal":"t9","type":"created"}}"#;
    assert!(
        matches!(update(created), HerdrUpdate::Created { ref terminal, .. }
        if terminal.address == "t9" && !terminal.live && terminal.status.is_none())
    );
}

/// A kept terminal whose program died: `lost`, then `restarted` with a new program (as hc
/// 0.1.22 sent them for a probe killed by SIGKILL).
#[test]
fn a_lost_terminal_that_restarts_is_live_again() {
    let mut t = HerdrTable::default();
    t.apply(update(SNAPSHOT), now());
    t.apply(update(CREATED), now());
    t.apply(update(WORKING), now());
    let lost = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"exit_code":null,"finished_ms":null,"reason":"signaled","signal":9},"seq":1240,"terminal":"t4x","type":"lost"}}"#;
    t.apply(update(lost), now());
    assert_eq!(
        herdr::state_of(t.terminals.get("t4x")),
        ThreadState::Idle,
        "its word went with it"
    );
    // Seen as gone meanwhile (a missed finished, say): restarted makes it live again.
    t.terminals.get_mut("t4x").unwrap().live = false;
    let restarted = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"address":"t4x","argv":["/bin/zsh","-l"],"cwd":"/tmp","state":"live","status":null,"status_ms":null,"title":"","name":"airboss-probe-k"},"seq":1241,"terminal":"t4x","type":"restarted"}}"#;
    t.apply(update(restarted), now());
    assert_eq!(herdr::state_of(t.terminals.get("t4x")), ThreadState::Idle);
    assert!(t.terminals["t4x"].live);
    // A status word means a program runs there, so it's live too.
    t.terminals.get_mut("t4x").unwrap().live = false;
    let working = WORKING.replace("1230", "1242");
    t.apply(update(&working), now());
    assert_eq!(
        herdr::state_of(t.terminals.get("t4x")),
        ThreadState::Running
    );
}

#[test]
fn the_captured_stream_applies_in_order() {
    let mut t = HerdrTable::default();
    for line in [SNAPSHOT, CREATED, FOREGROUND, WORKING] {
        if let Some(u) = parse_line(line).unwrap() {
            t.apply(u, now());
        }
    }
    let probe = |t: &HerdrTable| herdr::state_of(t.terminals.get("t4x"));
    assert_eq!(probe(&t), ThreadState::Running);
    t.apply(update(BLOCKED), now());
    assert_eq!(probe(&t), ThreadState::NeedsMe);
    t.apply(update(TITLE), now());
    t.apply(update(CLEARED), now());
    assert_eq!(probe(&t), ThreadState::Idle, "no status: idle");
    t.apply(update(FINISHED), now());
    assert_eq!(probe(&t), ThreadState::Gone);
    assert_eq!(
        herdr::state_of(t.terminals.get("t4p")),
        ThreadState::Idle,
        "done"
    );
    let views = herdr::views(&t, &["t4x".to_string(), "t4p".to_string()]);
    assert_eq!(views[0].title.as_deref(), Some("probe title"));
    assert_eq!(views[0].state, ThreadState::Gone);
    assert_eq!(views[1].project_path.as_deref(), Some("/Users/me/work"));
    assert_eq!(
        views[1].last_activity_at.as_deref(),
        Some("2026-11-25T07:48:24.598Z")
    );
}

#[test]
fn an_unknown_status_word_is_idle() {
    let mut t = HerdrTable::default();
    t.apply(update(SNAPSHOT), now());
    let line = WORKING
        .replace("\"working\"", "\"thinking\"")
        .replace("t4x", "t4p");
    t.apply(update(&line), now());
    assert_eq!(herdr::state_of(t.terminals.get("t4p")), ThreadState::Idle);
}

fn blocked(address: &str, kind: Option<&str>, msg: Option<&str>) -> String {
    let mut status = serde_json::json!({"app": "claude-code", "state": "blocked"});
    if let Some(k) = kind {
        status["kind"] = k.into();
    }
    if let Some(m) = msg {
        status["msg"] = m.into();
    }
    serde_json::json!({"seq": 1, "terminals": [{"address": address, "state": "live",
        "status": status, "status_ms": 1_795_600_000_000_u64, "title": "", "cwd": "/w"}]})
    .to_string()
}

fn status_event(seq: u64, address: &str, status: serde_json::Value) -> String {
    serde_json::json!({"jsonrpc": "2.0", "method": "event", "params": {"data": {"records": {},
        "reported": true, "status": status}, "seq": seq, "terminal": address, "type": "status"}})
    .to_string()
}

fn request_of(t: &HerdrTable, address: &str) -> airboss_services::Request {
    herdr::views(t, &[address.to_string()])[0]
        .request
        .clone()
        .unwrap()
}

#[test]
fn a_blocked_terminal_asks_what_its_program_said() {
    let cases = [
        (
            Some("permission"),
            Some("  trust this folder \n"),
            RequestKind::Approval,
            "trust this folder",
        ),
        (
            Some("question"),
            Some("Which branch?"),
            RequestKind::Question,
            "Which branch?",
        ),
        (
            Some("auth"),
            Some("Sign in"),
            RequestKind::Question,
            "Sign in",
        ),
        (
            Some("permission"),
            None,
            RequestKind::Question,
            herdr::NO_MESSAGE,
        ),
        (
            Some("permission"),
            Some("   "),
            RequestKind::Question,
            herdr::NO_MESSAGE,
        ),
        (None, None, RequestKind::Question, herdr::NO_MESSAGE),
    ];
    for (kind, msg, want_kind, want_text) in cases {
        let mut t = HerdrTable::default();
        t.apply(update(&blocked("t4p", kind, msg)), now());
        let r = request_of(&t, "t4p");
        assert_eq!(
            (r.kind, r.text.as_str()),
            (want_kind, want_text),
            "{kind:?} {msg:?}"
        );
        assert_eq!(r.id, "herdr/t4p/1795600000000");
        assert_eq!(r.at, "2026-11-25T09:46:40.000Z");
    }
    // Only a blocked terminal asks.
    let mut t = HerdrTable::default();
    t.apply(update(SNAPSHOT), now());
    assert_eq!(
        herdr::views(&t, &["t4p".to_string()])[0].request,
        None,
        "done"
    );
}

#[test]
fn a_request_keeps_its_id_when_the_stream_restarts() {
    let mut t = HerdrTable::default();
    t.apply(update(&blocked("t4p", None, None)), now());
    // It becomes blocked on a permission through an event, stamped when it arrives.
    let asks =
        serde_json::json!({"state": "blocked", "kind": "permission", "msg": "trust this folder"});
    t.apply(update(&status_event(2, "t4p", asks)), now());
    let first = request_of(&t, "t4p");
    assert_eq!(first.id, "herdr/t4p/1795672000000");
    assert_eq!(first.kind, RequestKind::Approval);
    // The restarted stream's snapshot has hc's own time for the word: the id stays.
    let later = now() + Duration::from_secs(30);
    let restart = blocked("t4p", Some("permission"), Some("trust this folder"));
    assert!(!t.apply(update(&restart), later), "nothing changed");
    assert_eq!(request_of(&t, "t4p"), first);
    // A created or restarted record with the same status keeps it too.
    let record = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"address":"t4p","state":"live","status":{"state":"blocked","kind":"permission","msg":"trust this folder"},"status_ms":5,"title":"","cwd":"/w"},"seq":3,"terminal":"t4p","type":"restarted"}}"#;
    t.apply(update(record), later);
    assert_eq!(request_of(&t, "t4p").id, first.id);
}

#[test]
fn a_new_message_while_blocked_is_a_new_request() {
    let mut t = HerdrTable::default();
    t.apply(
        update(&blocked(
            "t4p",
            Some("permission"),
            Some("trust this folder"),
        )),
        now(),
    );
    let first = request_of(&t, "t4p");
    let later = now() + Duration::from_secs(5);
    let next = serde_json::json!({"state": "blocked", "kind": "permission", "msg": "run npm test"});
    assert!(t.apply(update(&status_event(2, "t4p", next)), later));
    let second = request_of(&t, "t4p");
    assert_eq!(second.text, "run npm test");
    assert_ne!(second.id, first.id);
    assert_eq!(second.id, "herdr/t4p/1795672005000");
    // Seen first in a snapshot (the event was missed), hc's time for the word hasn't moved;
    // the message did, so it's a new request too.
    let much_later = later + Duration::from_secs(5);
    let snap = blocked("t4p", Some("question"), Some("Which branch?"));
    t.apply(update(&snap), much_later);
    let third = request_of(&t, "t4p");
    assert_eq!(third.id, "herdr/t4p/1795672010000");
    assert_eq!(third.kind, RequestKind::Question);
}

#[test]
fn herdr_threads_have_no_url() {
    let key = ThreadKey {
        source: Source::Herdr,
        r#ref: "t4p".into(),
    };
    assert_eq!(
        airboss_sync::threads::thread_url(Some("0b8a1f9e-1d2c-4b3a-9f8e-7d6c5b4a3f21"), &key),
        None
    );
}

// ---- the real source, with a script in place of `hc` ----

/// Writes an executable `hc` into `dir` that prints `lines`, then runs `tail`.
fn fake_hc(dir: &Path, lines: &[&str], tail: &str) -> PathBuf {
    let path = dir.join("hc");
    let mut body = String::from(
        "#!/bin/sh\n# stdin must be closed: this returns at once.\nread -r _ignored\n",
    );
    body.push_str("echo \"{\\\"seq\\\":0,\\\"terminals\\\":[{\\\"address\\\":\\\"args\\\",\\\"state\\\":\\\"live\\\",\\\"title\\\":\\\"$*\\\"}]}\"\n");
    for line in lines {
        body.push_str(&format!("printf '%s\\n' '{line}'\n"));
    }
    body.push_str(tail);
    body.push('\n');
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn feeds(rx: &Receiver<T3Msg>, until_end: bool, n: usize) -> Vec<(u64, HerdrFeed)> {
    let mut out = Vec::new();
    while out.len() < n {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(T3Msg::Herdr { stream, feed }) => {
                let end = matches!(feed, HerdrFeed::Ended(_));
                out.push((stream, feed));
                if end && until_end {
                    break;
                }
            }
            Ok(other) => panic!("unexpected {other:?}"),
            Err(e) => panic!("no line within 5 s: {e}; so far {out:?}"),
        }
    }
    out
}

/// Whether a process with this pid still exists, zombies included.
fn exists(pid: u32) -> bool {
    let out = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    !String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

#[test]
fn the_real_source_reads_lines_until_hc_exits_and_reaps_it() {
    let dir = TempDir::new();
    let hc = fake_hc(dir.path(), &[SNAPSHOT, HEARTBEAT, DOC, WORKING], "exit 0");
    let mut src = RealHerdr::new(&hc);
    let (tx, rx) = channel();
    src.follow(7, tx).unwrap();
    let pid = src.pid().unwrap();
    let got = feeds(&rx, true, 10);
    assert!(got.iter().all(|(s, _)| *s == 7), "tagged with the stream");
    let updates: Vec<&HerdrFeed> = got.iter().map(|(_, f)| f).collect();
    assert_eq!(updates.len(), 4, "heartbeat and doc dropped: {updates:?}");
    match updates[0] {
        HerdrFeed::Update(HerdrUpdate::Snapshot { terminals, .. }) => {
            assert_eq!(terminals[0].title.as_deref(), Some("events --json"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        updates[1],
        HerdrFeed::Update(HerdrUpdate::Snapshot { seq: 1227, .. })
    ));
    assert!(matches!(
        updates[2],
        HerdrFeed::Update(HerdrUpdate::Status { .. })
    ));
    assert_eq!(updates[3], &HerdrFeed::Ended("hc events stopped".into()));
    // What the worker does on the end: stop, which reaps the child.
    src.stop();
    assert_eq!(src.pid(), None);
    assert!(!exists(pid), "no zombie left");
}

#[test]
fn stopping_kills_a_running_hc_and_waits_for_it() {
    let dir = TempDir::new();
    let hc = fake_hc(dir.path(), &[SNAPSHOT], "exec sleep 30");
    let mut src = RealHerdr::new(&hc);
    let (tx, rx) = channel();
    src.follow(1, tx).unwrap();
    let pid = src.pid().unwrap();
    feeds(&rx, false, 2);
    assert!(exists(pid));
    src.stop();
    assert!(!exists(pid), "killed and reaped");
    let end = feeds(&rx, true, 1);
    assert_eq!(end[0], (1, HerdrFeed::Ended("hc events stopped".into())));
}

#[test]
fn following_again_replaces_the_stream_and_dropping_stops_it() {
    let dir = TempDir::new();
    let hc = fake_hc(dir.path(), &[], "exec sleep 30");
    let mut src = RealHerdr::new(&hc);
    let (tx, rx) = channel();
    src.follow(1, tx.clone()).unwrap();
    let first = src.pid().unwrap();
    src.follow(2, tx).unwrap();
    let second = src.pid().unwrap();
    assert!(!exists(first), "the old child is gone");
    assert!(exists(second));
    drop(src);
    assert!(!exists(second), "dropping the source kills its child");
    let mut ends: Vec<u64> = Vec::new();
    while let Ok(T3Msg::Herdr { stream, feed }) = rx.recv_timeout(Duration::from_secs(5)) {
        if matches!(feed, HerdrFeed::Ended(_)) {
            ends.push(stream);
        }
        if ends.len() == 2 {
            break;
        }
    }
    ends.sort_unstable();
    assert_eq!(ends, vec![1, 2]);
}

#[test]
fn a_line_sync_cant_read_ends_the_stream() {
    let dir = TempDir::new();
    let hc = fake_hc(dir.path(), &["garbage"], "exec sleep 30");
    let mut src = RealHerdr::new(&hc);
    let (tx, rx) = channel();
    src.follow(1, tx).unwrap();
    let got = feeds(&rx, true, 3);
    match &got.last().unwrap().1 {
        HerdrFeed::Ended(why) => assert!(why.contains("isn't JSON"), "{why}"),
        other => panic!("{other:?}"),
    }
    src.stop();
}
