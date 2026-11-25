//! Claude Code sessions from fake session files: states, a reused pid, an unknown status,
//! and `t3-code` sessions never untracked.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;

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
    let sessions = fake.clone().sessions().unwrap();
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
    let s = fake.sessions().unwrap();
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(
        &s,
        &alive,
        &ids(&["busy-one", "idle-one", "never-seen"]),
        &set(&["busy-one", "idle-one", "never-seen"]),
    )
    .unwrap();
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
    let s = fake.sessions().unwrap();
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(&s, &alive, &ids(&["s-7"]), &set(&["s-7"])).unwrap();
    assert_eq!(v[0].state, ThreadState::Gone);
}

#[test]
fn ps_output_must_match_proc_start() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(3080, "a", "busy", "cli");
    fake.write(75242, "b", "busy", "cli");
    fake.write(9, "c", "busy", "cli");
    let s = fake.sessions().unwrap();
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
fn an_unknown_status_is_an_error_naming_it() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    fake.write(1, "s", "thinking", "cli");
    let s = fake.sessions().unwrap();
    let err = claude::views(&s, &[true], &[], &HashSet::new()).unwrap_err();
    assert_eq!(err, "unknown session status \"thinking\"");
}

#[test]
fn a_missing_required_field_is_invalid_data_and_a_torn_file_is_skipped() {
    let dir = TempDir::new();
    let mut fake = FakeClaude::new(dir.path());
    std::fs::write(dir.path().join("5.json"), "{\"pid\": 5, \"sess").unwrap();
    assert!(fake.sessions().unwrap().is_empty(), "caught mid-write");
    std::fs::write(
        dir.path().join("6.json"),
        r#"{"pid": 6, "sessionId": "x", "status": "busy"}"#,
    )
    .unwrap();
    let err = fake.sessions().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("procStart"));
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
    let s = fake.sessions().unwrap();
    let alive = fake.alive(&s).unwrap();
    let v = claude::views(&s, &alive, &[], &set(&["linked-elsewhere"])).unwrap();
    let untracked: Vec<&str> = v.iter().map(|t| t.key.r#ref.as_str()).collect();
    assert_eq!(untracked, vec!["from-cli"]);
    assert!(!v[0].linked);
    assert_eq!(v[0].state, ThreadState::Idle);

    // Linked, a t3-code session is reported like any other.
    let v = claude::views(&s, &alive, &ids(&["from-t3"]), &set(&["from-t3"])).unwrap();
    assert_eq!(v[0].key.r#ref, "from-t3");
    assert_eq!(v[0].state, ThreadState::Running);
}
