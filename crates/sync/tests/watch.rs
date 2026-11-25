//! The real kqueue watcher on a WAL database another process keeps open and writes to.
//!
//! The child is this test binary run again with `AIRBOSS_WATCH_CHILD` set: it opens
//! `statev2.sqlite` in WAL mode, keeps the connection open, and on each line from the
//! parent writes, closes (which deletes the WAL), or reopens.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use airboss_sync::fake::TempDir;
use airboss_sync::watch::KqueueWatch;
use airboss_sync::{T3Msg, WalWatch, WatchTarget};

const CHILD: &str = "AIRBOSS_WATCH_CHILD";

/// Not a test of its own: the child process's body. Without the variable it does nothing.
#[test]
fn child_writer() {
    let Ok(dir) = std::env::var(CHILD) else {
        return;
    };
    let db = Path::new(&dir).join("statev2.sqlite");
    let open = || {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
        c.execute_batch("CREATE TABLE IF NOT EXISTS t (n INTEGER)")
            .unwrap();
        c
    };
    let mut conn = Some(open());
    let mut out = std::io::stdout();
    writeln!(out, "ready").unwrap();
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "write" => {
                conn.as_ref()
                    .unwrap()
                    .execute("INSERT INTO t VALUES (1)", [])
                    .unwrap();
            }
            "close" => drop(conn.take()),
            "open" => {
                let c = open();
                c.execute("INSERT INTO t VALUES (2)", []).unwrap();
                conn = Some(c);
            }
            _ => break,
        }
        writeln!(out, "ok").unwrap();
        out.flush().unwrap();
    }
}

/// Waits up to 1 s for `want`, answering folder events the way the T3 worker does (by
/// calling `watch` again). Returns whether `want` came.
fn expect(
    rx: &Receiver<T3Msg>,
    w: &mut KqueueWatch,
    targets: &[WatchTarget],
    tx: &std::sync::mpsc::Sender<T3Msg>,
    want: fn(&T3Msg) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(msg) => {
                if matches!(msg, T3Msg::T3Dir) {
                    w.watch(targets, tx.clone()).unwrap();
                }
                if want(&msg) {
                    return true;
                }
            }
            Err(_) => return false,
        }
    }
}

fn drain(
    rx: &Receiver<T3Msg>,
    w: &mut KqueueWatch,
    targets: &[WatchTarget],
    tx: &std::sync::mpsc::Sender<T3Msg>,
) {
    std::thread::sleep(Duration::from_millis(100));
    while let Ok(msg) = rx.try_recv() {
        if matches!(msg, T3Msg::T3Dir) {
            w.watch(targets, tx.clone()).unwrap();
        }
    }
}

#[test]
fn kqueue_reports_every_wal_write_from_another_process_and_rearms() {
    if std::env::var(CHILD).is_ok() {
        return;
    }
    let dir = TempDir::new();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_writer", "--nocapture", "--test-threads=1"])
        .env(CHILD, dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut to_child = child.stdin.take().unwrap();
    let mut from_child = BufReader::new(child.stdout.take().unwrap());
    let mut say = |cmd: &str| {
        writeln!(to_child, "{cmd}").unwrap();
        to_child.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                from_child.read_line(&mut line).unwrap() > 0,
                "the child ended"
            );
            if line.trim() == "ok" {
                return;
            }
        }
    };
    let wal = dir.path().join("statev2.sqlite-wal");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !wal.exists() {
        assert!(Instant::now() < deadline, "the child never made its WAL");
        std::thread::sleep(Duration::from_millis(20));
    }

    let targets = vec![
        WatchTarget::T3Dir(dir.path().to_path_buf()),
        WatchTarget::T3Wal(wal.clone()),
    ];
    let (tx, rx) = channel();
    let mut w = KqueueWatch::new();
    w.watch(&targets, tx.clone()).unwrap();
    drain(&rx, &mut w, &targets, &tx);

    for i in 0..5 {
        say("write");
        assert!(
            expect(&rx, &mut w, &targets, &tx, |m| matches!(m, T3Msg::Wal)),
            "write {i} reported within 1 s"
        );
        drain(&rx, &mut w, &targets, &tx);
    }

    say("close");
    let deadline = Instant::now() + Duration::from_secs(2);
    while wal.exists() {
        assert!(
            Instant::now() < deadline,
            "closing the last connection deletes the WAL"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    drain(&rx, &mut w, &targets, &tx);

    // Reopened: the WAL is created again; the folder event re-arms its watch.
    say("open");
    assert!(
        expect(&rx, &mut w, &targets, &tx, |m| matches!(m, T3Msg::T3Dir)),
        "the new WAL shows as a folder event"
    );
    drain(&rx, &mut w, &targets, &tx);
    say("write");
    assert!(
        expect(&rx, &mut w, &targets, &tx, |m| matches!(m, T3Msg::Wal)),
        "the re-armed watch reports writes"
    );

    say("exit");
    let _ = child.wait();
}
