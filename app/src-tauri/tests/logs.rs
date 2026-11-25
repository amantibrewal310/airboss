//! R1: every service's log line, and a panic on any thread, reach the app's log (and so
//! `~/Library/Logs/Airboss/app.log`), not stderr, which is /dev/null when the app opens
//! from Finder. And no code in the app process writes to stderr behind the log's back.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri_plugin_log::log::{self, Level, Log, Metadata, Record};

/// Stands in for the log plugin: the records the app log would write.
struct Capture(Mutex<Vec<(Level, String, String)>>);

impl Log for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn log(&self, r: &Record<'_>) {
        self.0
            .lock()
            .unwrap()
            .push((r.level(), r.target().to_string(), r.args().to_string()));
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn records() -> Vec<(Level, String, String)> {
    CAPTURE.0.lock().unwrap().clone()
}

#[test]
fn service_lines_and_panics_go_to_the_app_log() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    airboss_app::logs::install();

    // What sync, launch and attention call for each line they log.
    airboss_services::log::line("airboss-sync", "Claude sync broken: ps failed");
    let r = records();
    assert!(
        r.contains(&(
            Level::Warn,
            "airboss-sync".into(),
            "Claude sync broken: ps failed".into()
        )),
        "{r:?}"
    );

    // A panic on a service thread: its message and where, as an error.
    let joined = std::thread::Builder::new()
        .name("airboss-sync-t3".into())
        .spawn(|| panic!("T3's database moved"))
        .unwrap()
        .join();
    assert!(joined.is_err());
    let panics: Vec<_> = records()
        .into_iter()
        .filter(|(level, target, _)| *level == Level::Error && target == "panic")
        .collect();
    assert_eq!(panics.len(), 1, "{panics:?}");
    let text = &panics[0].2;
    assert!(text.contains("airboss-sync-t3"), "{text}");
    assert!(text.contains("T3's database moved"), "{text}");
    assert!(text.contains("tests/logs.rs"), "{text}");
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn nothing_in_the_app_process_writes_to_stderr_directly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [
        "crates/core/src",
        "crates/services/src",
        "crates/sync/src",
        "crates/launch/src",
        "crates/attention/src",
        "app/src-tauri/src",
    ] {
        sources(&root.join(dir), &mut files);
    }
    assert!(files.len() > 40, "found only {} files", files.len());
    // The services' fallback before the app installs its sink, and `main`'s last word when
    // Tauri itself can't start (the log may not exist yet).
    let allowed = ["crates/services/src/log.rs", "app/src-tauri/src/main.rs"];
    let mut offenders = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if allowed.contains(&rel.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if ["eprintln!", "eprint!", "io::stderr", "dbg!"]
                .iter()
                .any(|p| code.contains(p))
            {
                offenders.push(format!("{rel}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these write to stderr, which is lost when the app opens from Finder; \
         use airboss_services::log::line or the log crate:\n{}",
        offenders.join("\n")
    );
}
