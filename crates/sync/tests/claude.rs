//! Claude Code sessions from fake session files: states (`waiting` too), a reused pid, an
//! unknown status, files caught mid-write, and `t3-code` sessions never untracked.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use airboss_services::{Source, ThreadState};
use airboss_sync::claude::{self, ClaudeSource};
use airboss_sync::fake::{FakeClaude, TempDir};

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn set(v: &[&str]) -> HashSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn files_are_read_and_key_files_never_are() {
    let dir = TempDir::new();
    let fake = FakeClaude::new(dir.path());
    fake.write(3080, "s-1", "busy", "cli");
    std::fs::write(dir.path().join("3080.69fb.key"), "secret").unwrap();
    std::fs::write(dir.path().join("notes.json"), "{}").unwrap();
    let sessions = fake.clone().sessions().unwrap().sessions;
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!((s.pid, s.session_id.as_str()), (3080, "s-1"));
    assert_eq!(s.proc_start, "Wed Nov 25 17:19:03 2026");
    assert_eq!(s.cwd.as_deref(), Some("/work/web"));
}

#[test]
fn states() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(1, "busy-one", "busy", "cli");
    fake.write(2, "idle-one", "idle", "cli");
    let s = fake.sessions().unwrap().sessions;
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(
        &s,
        &alive,
        &ids(&["busy-one", "idle-one", "never-seen"]),
        &set(&["busy-one", "idle-one", "never-seen"]),
    );
    let state = |id: &str| v.iter().find(|t| t.key.r#ref == id).unwrap().state;
    assert_eq!(state("busy-one"), ThreadState::Running);
    assert_eq!(state("idle-one"), ThreadState::Idle);
    assert_eq!(state("never-seen"), ThreadState::Gone, "no file");
    let busy = v.iter().find(|t| t.key.r#ref == "busy-one").unwrap();
    assert_eq!(busy.key.source, Source::Claude);
    assert_eq!(busy.title.as_deref(), Some("session 1"));
    assert_eq!(
        busy.last_activity_at.as_deref(),
        Some("2026-11-25T09:46:41.000Z"),
        "the later of updatedAt and statusUpdatedAt"
    );
}

#[test]
fn a_dead_or_reused_pid_is_gone() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(7, "s-7", "busy", "cli");
    fake.kill(7);
    let s = fake.sessions().unwrap().sessions;
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(&s, &alive, &ids(&["s-7"]), &set(&["s-7"]));
    assert_eq!(v[0].state, ThreadState::Gone);
}

#[test]
fn ps_output_must_match_proc_start() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(3080, "a", "busy", "cli");
    fake.write(75242, "b", "busy", "cli");
    fake.write(9, "c", "busy", "cli");
    let s = fake.sessions().unwrap().sessions;
    // 3080 is the same process; 75242 is a reused pid (another start time); 9 is gone.
    let ps = "  3080 Wed Nov 25 17:19:03 2026    \n75242 Thu Nov 26 09:00:00 2026\n";
    let by_pid: Vec<(u32, bool)> = s
        .iter()
        .map(|x| x.pid)
        .zip(claude::parse_ps(ps, &s))
        .collect();
    assert!(by_pid.contains(&(3080, true)));
    assert!(by_pid.contains(&(75242, false)));
    assert!(by_pid.contains(&(9, false)));
}

#[test]
fn waiting_is_needs_me() {
    assert_eq!(claude::state_of("waiting"), ThreadState::NeedsMe);
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(1, "asks", "waiting", "cli");
    fake.write(2, "untracked", "waiting", "cli");
    let s = fake.sessions().unwrap().sessions;
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(&s, &alive, &ids(&["asks"]), &set(&["asks"]));
    let state = |id: &str| v.iter().find(|t| t.key.r#ref == id).unwrap().state;
    assert_eq!(state("asks"), ThreadState::NeedsMe);
    assert_eq!(state("untracked"), ThreadState::NeedsMe);
    let asks = v.iter().find(|t| t.key.r#ref == "asks").unwrap();
    assert!(
        asks.request.is_none(),
        "the file says only what it waits for"
    );
}

#[test]
fn shell_is_working_and_an_unknown_status_is_idle() {
    // Claude Code writes `shell` when its turn has ended but a background shell task still
    // runs; its own session list counts that as busy.
    assert_eq!(claude::state_of("shell"), ThreadState::Running);
    // Claude Code adds statuses over time: a new one shows the session as idle and leaves
    // every other session, and Claude sync, as they were.
    assert_eq!(claude::state_of("thinking"), ThreadState::Idle);
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(1, "background", "shell", "cli");
    fake.write(2, "newer", "thinking", "cli");
    fake.write(3, "plain", "busy", "cli");
    let s = fake.sessions().unwrap().sessions;
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(&s, &alive, &ids(&["newer"]), &set(&["newer"]));
    let state = |id: &str| v.iter().find(|t| t.key.r#ref == id).unwrap().state;
    assert_eq!(state("background"), ThreadState::Running);
    assert_eq!(state("newer"), ThreadState::Idle);
    assert_eq!(state("plain"), ThreadState::Running);
    assert_eq!(
        claude::unknown_statuses(&s),
        vec!["thinking"],
        "named for the log"
    );
}

#[test]
fn a_torn_file_is_skipped() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    std::fs::write(dir.path().join("5.json"), "{\"pid\": 5, \"sess").unwrap();
    let read = fake.sessions().unwrap();
    assert!(read.sessions.is_empty(), "caught mid-write");
    assert_eq!(
        read.skipped,
        vec![dir.path().join("5.json")],
        "still watched"
    );
}

/// Sets a file's mtime `secs` seconds back.
fn age(path: &Path, secs: u64) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn a_fresh_file_missing_a_required_field_is_skipped_for_now() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(1, "ok", "busy", "cli");
    // Claude Code writes a new session's file before its status.
    for (pid, json) in [
        (6, r#"{"pid": 6, "sessionId": "x", "procStart": "p"}"#),
        (7, r#"{"pid": 7, "status": "busy", "procStart": "p"}"#),
        (8, r#"{"pid": 8, "sessionId": "x", "status": "busy"}"#),
    ] {
        std::fs::write(dir.path().join(format!("{pid}.json")), json).unwrap();
    }
    let read = fake.sessions().unwrap();
    let ids: Vec<&str> = read
        .sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["ok"]);
    let skipped: Vec<PathBuf> = [6, 7, 8]
        .iter()
        .map(|p| dir.path().join(format!("{p}.json")))
        .collect();
    assert_eq!(
        read.skipped, skipped,
        "watched, so the next write reads them"
    );
}

#[test]
fn a_file_still_missing_a_required_field_once_not_fresh_is_invalid_data() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    for (pid, json, field) in [
        (
            6,
            r#"{"pid": 6, "sessionId": "x", "procStart": "p"}"#,
            "status",
        ),
        (
            7,
            r#"{"pid": 7, "status": "busy", "procStart": "p"}"#,
            "sessionId",
        ),
        (
            8,
            r#"{"pid": 8, "sessionId": "x", "status": "busy"}"#,
            "procStart",
        ),
    ] {
        let file = dir.path().join(format!("{pid}.json"));
        std::fs::write(&file, json).unwrap();
        age(&file, 11);
        let err = fake.sessions().unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            err.to_string(),
            format!("Claude session file {} has no {field}", file.display())
        );
        std::fs::remove_file(&file).unwrap();
    }
}

#[test]
fn a_missing_folder_is_not_found() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(&dir.path().join("nope"));
    assert_eq!(
        fake.sessions().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
}

#[test]
fn t3_code_sessions_are_never_untracked() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(1, "from-t3", "busy", "t3-code");
    fake.write(2, "from-cli", "idle", "cli");
    fake.write(3, "linked-elsewhere", "idle", "cli");
    let s = fake.sessions().unwrap().sessions;
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(&s, &alive, &[], &set(&["linked-elsewhere"]));
    let untracked: Vec<&str> = v.iter().map(|t| t.key.r#ref.as_str()).collect();
    assert_eq!(untracked, vec!["from-cli"]);
    assert!(!v[0].linked);
    assert_eq!(v[0].state, ThreadState::Idle);

    // Linked, a t3-code session is reported like any other.
    let v = claude::views(&s, &alive, &ids(&["from-t3"]), &set(&["from-t3"]));
    assert_eq!(v[0].key.r#ref, "from-t3");
    assert_eq!(v[0].state, ThreadState::Running);
}
