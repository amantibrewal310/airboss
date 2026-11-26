//! Against this Mac's real `hc` (`#[ignore]`, run by hand):
//!
//! ```sh
//! cargo test -p airboss-sync --test real_herdr -- --ignored
//! ```
//!
//! It follows `hc events --json` with the real source, starts one harmless terminal of its
//! own (`airboss-probe-<pid>`, a shell script reporting OSC 7501 statuses), checks each state
//! arrives, then kills and removes that terminal. It never touches any other terminal.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant, SystemTime};

use airboss_services::ThreadState;
use airboss_sync::T3Msg;
use airboss_sync::fake::TempDir;
use airboss_sync::herdr::{self, HerdrFeed, HerdrSource, HerdrTable, HerdrUpdate, RealHerdr};

fn hc() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME");
    Path::new(&home).join(".local/bin/hc")
}

/// Kills and removes the probe terminal, however the test ends.
struct Probe(String);

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = Command::new(hc()).args(["kill", &self.0]).output();
        let _ = Command::new(hc()).args(["rm", &self.0]).output();
    }
}

const SCRIPT: &str = r#"#!/bin/sh
# Reports each status Decision 6 maps, as a program does: OSC 7501, `key=value` pairs.
sleep 1
printf '\033]7501;state=working\007'; sleep 1
printf '\033]7501;state=blocked:kind=question\007'; sleep 1
printf '\033]7501;state=error\007'; sleep 1
printf '\033]7501;state=done\007'; sleep 1
exit 0
"#;

#[test]
#[ignore = "runs this Mac's real hc and one terminal of its own; run by hand"]
fn the_real_hc_reports_each_state_of_a_probe_terminal() {
    assert!(hc().is_file(), "no hc at {}", hc().display());
    let dir = TempDir::new();
    let script = dir.path().join("probe.sh");
    std::fs::write(&script, SCRIPT).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut src = RealHerdr::new(&hc());
    let (tx, rx) = channel();
    src.follow(1, tx).unwrap();
    let pid = src.pid().unwrap();
    let mut table = HerdrTable::default();
    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let T3Msg::Herdr {
        feed: HerdrFeed::Update(u @ HerdrUpdate::Snapshot { .. }),
        ..
    } = first
    else {
        panic!("the first line isn't a snapshot: {first:?}");
    };
    table.apply(u, SystemTime::now());

    let name = format!("airboss-probe-{}", std::process::id());
    let out = Command::new(hc())
        .args(["run", "--bg", "--json", "--no-parent", "--no-notify"])
        .args(["--name", &name, "--label", "airboss-probe=1", "--cwd"])
        .arg(dir.path())
        .arg("--")
        .arg(&script)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let started: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let address = started["address"].as_str().unwrap().to_string();
    let _probe = Probe(address.clone());

    let mut seen: Vec<ThreadState> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let Ok(msg) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) else {
            break;
        };
        let T3Msg::Herdr { feed, .. } = msg else {
            continue;
        };
        match feed {
            HerdrFeed::Update(u) => {
                table.apply(u, SystemTime::now());
            }
            HerdrFeed::Ended(why) => panic!("the stream ended: {why}"),
        }
        let state = herdr::state_of(table.terminals.get(&address));
        if seen.last() != Some(&state) {
            seen.push(state);
        }
        if state == ThreadState::Gone && seen.len() > 1 {
            break;
        }
    }
    let views = herdr::views(&table, std::slice::from_ref(&address));
    assert_eq!(
        views[0].title.as_deref(),
        Some(name.as_str()),
        "no title: its name"
    );
    assert_eq!(
        views[0].project_path.as_deref(),
        Some(dir.path().to_str().unwrap())
    );
    assert_eq!(
        seen,
        vec![
            ThreadState::Idle,
            ThreadState::Running,
            ThreadState::NeedsMe,
            ThreadState::Errored,
            ThreadState::Idle,
            ThreadState::Gone,
        ]
    );
    src.stop();
    let ps = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&ps.stdout).trim().is_empty(),
        "hc events is killed and reaped"
    );
}
