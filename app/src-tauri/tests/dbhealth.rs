//! R2: a database that breaks while the app runs shows the "database problem" state
//! (`db_unavailable`) instead of failing silently. Two cheap signs: a read or write failing
//! with an I/O or corruption error, and the `-wal` file vanishing or being replaced while the
//! app holds it. SQLite isn't repaired.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;
use std::time::Duration;

use airboss_app::UiEvent;
use airboss_app::catchup::Trigger;
use airboss_app::errors::{AppError, ErrorCode};
use airboss_app::watcher::wal_path;
use airboss_core::{Actor, NewTask, Store};
use common::{TestApp, temp_dir};
use rusqlite::ffi;
use serde_json::json;

fn file_app(name: &str) -> (TestApp, PathBuf) {
    let db = temp_dir(name).join("airboss.db");
    let app = TestApp::with_store(Store::open(&db).unwrap());
    app.state.start_background();
    (app, db)
}

fn problem(app: &TestApp, within: Duration) -> Option<AppError> {
    match app
        .shell
        .wait_for(within, |e| matches!(e, UiEvent::DbUnavailable(_)))
    {
        Some(UiEvent::DbUnavailable(e)) => Some(e),
        _ => None,
    }
}

fn problems(app: &TestApp) -> usize {
    app.shell
        .events()
        .iter()
        .filter(|e| matches!(e, UiEvent::DbUnavailable(_)))
        .count()
}

#[test]
fn deleting_the_live_wal_shows_the_database_problem() {
    let (app, db) = file_app("wal-deleted");
    app.call("bootstrap", json!({})).unwrap();
    let wal = wal_path(&db);
    assert!(wal.exists(), "the app holds the WAL open");
    std::fs::remove_file(&wal).unwrap();

    let e = problem(&app, Duration::from_secs(1)).expect("db:unavailable within a second");
    assert_eq!(e.code, ErrorCode::DbUnavailable);
    let detail = e.detail.clone().unwrap();
    let path = app.core(|s| s.path()).unwrap();
    assert!(
        detail.starts_with(&format!("{}: ", path.display())),
        "{detail}"
    );
    assert!(detail.contains("airboss.db-wal"), "{detail}");
    assert!(e.message.contains("Quit Airboss"), "{}", e.message);

    // Every command now answers with it, so a reload lands on the same state …
    let boot = app.call("bootstrap", json!({})).unwrap_err();
    assert_eq!(boot, e);
    let write = app
        .call("create_task", json!({"task": {"title": "Lost?", "body": null, "parent": null, "repos": [], "links": [], "then": "none"}}))
        .unwrap_err();
    assert_eq!(write.code, ErrorCode::DbUnavailable);
    // … except what the state itself needs: Show in Finder, and showing the window.
    app.call("ui_ready", json!({"window": "main"})).unwrap();
    app.call(
        "open_url",
        json!({"target": {"url": format!("file://{}", db.parent().unwrap().display())}}),
    )
    .unwrap();

    // Said once, however many triggers follow.
    app.state.trigger(Trigger::Focus);
    app.state.trigger(Trigger::Core);
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(problems(&app), 1);
}

#[test]
fn a_replaced_wal_is_a_problem_too() {
    let (app, db) = file_app("wal-replaced");
    let wal = wal_path(&db);
    std::fs::remove_file(&wal).unwrap();
    std::fs::write(&wal, b"").unwrap();
    let e = problem(&app, Duration::from_secs(1)).expect("db:unavailable");
    assert!(e.detail.unwrap().contains("replaced"));
}

#[test]
fn normal_use_from_other_processes_is_not_a_problem() {
    let (app, db) = file_app("wal-healthy");
    // An agent opens, writes, checkpoints the WAL to nothing and closes, several times.
    for i in 0..3 {
        let mut agent = Store::open(&db).unwrap();
        agent
            .add_task(NewTask::new(format!("From an agent {i}")), &Actor::Me)
            .unwrap();
        drop(agent);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .unwrap();
    }
    app.state.trigger(Trigger::Focus);
    app.state.trigger(Trigger::Wake);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(problems(&app), 0);
    let boot = app.call("bootstrap", json!({})).unwrap();
    assert_eq!(boot["rows"].as_array().unwrap().len(), 3);
}

fn sqlite_error(code: i32, text: &str) -> airboss_core::Error {
    airboss_core::Error::Db(rusqlite::Error::SqliteFailure(
        ffi::Error::new(code),
        Some(text.into()),
    ))
}

#[test]
fn an_io_or_corruption_error_from_a_read_or_write_shows_the_database_problem() {
    for (code, text) in [
        (ffi::SQLITE_IOERR_SHORT_READ, "disk I/O error"),
        (ffi::SQLITE_CORRUPT, "database disk image is malformed"),
    ] {
        let app = TestApp::new();
        let r = app.state.core(|_| Err::<(), _>(sqlite_error(code, text)));
        assert!(r.is_err());
        let e = problem(&app, Duration::from_millis(100)).expect("db:unavailable");
        assert_eq!(e.code, ErrorCode::DbUnavailable);
        assert!(e.detail.as_deref().unwrap().contains(text), "{e:?}");
        assert_eq!(
            app.call("bootstrap", json!({})).unwrap_err().code,
            ErrorCode::DbUnavailable
        );
    }
}

#[test]
fn a_busy_database_is_only_a_failed_save() {
    let app = TestApp::new();
    let r = app
        .state
        .core(|_| Err::<(), _>(sqlite_error(ffi::SQLITE_BUSY, "database is locked")));
    assert!(r.is_err());
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(problems(&app), 0);
    app.call("bootstrap", json!({})).unwrap();
}
