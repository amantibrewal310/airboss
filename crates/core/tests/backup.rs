//! `Store::backup`: a consistent copy of the whole file, through SQLite's online backup.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::{Actor, Error, EventId, NewTask, Store};
use common::TempDir;

fn seeded(dir: &TempDir) -> Store {
    let mut s = Store::open(&dir.0.join("airboss.db")).unwrap();
    for title in ["One", "Two", "Three"] {
        s.add_task(NewTask::new(title), &Actor::Me).unwrap();
    }
    s
}

fn titles(s: &Store) -> Vec<String> {
    s.tree()
        .unwrap()
        .into_iter()
        .map(|n| n.task.title)
        .collect()
}

#[test]
fn a_backup_opens_as_the_same_tasks_and_history_and_leaves_nothing_else() {
    let dir = TempDir::new();
    let s = seeded(&dir);
    let out = TempDir::new();
    let copy = out.0.join("copy.db");
    s.backup(&copy, false).unwrap();
    let names: Vec<String> = std::fs::read_dir(&out.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["copy.db"], "no temp file, -wal or -journal");

    let b = Store::open(&copy).unwrap();
    assert_eq!(titles(&b), titles(&s));
    assert_eq!(
        b.events_after(EventId(0)).unwrap(),
        s.events_after(EventId(0)).unwrap()
    );
}

#[test]
fn a_backup_is_one_moment_and_leaves_out_what_another_handle_has_not_committed() {
    let dir = TempDir::new();
    let s = seeded(&dir);
    let other = rusqlite::Connection::open(s.path().unwrap()).unwrap();
    other
        .execute_batch(
            "BEGIN IMMEDIATE;
        INSERT INTO tasks (title, position, origin) VALUES ('Uncommitted', 4, 'me');",
        )
        .unwrap();
    let out = TempDir::new();
    let copy = out.0.join("copy.db");
    s.backup(&copy, false).unwrap();
    other.execute_batch("COMMIT;").unwrap();
    assert_eq!(
        titles(&Store::open(&copy).unwrap()),
        vec!["One", "Two", "Three"]
    );
}

#[test]
fn an_existing_file_is_kept_unless_overwrite_is_given() {
    let dir = TempDir::new();
    let mut s = seeded(&dir);
    let out = TempDir::new();
    let copy = out.0.join("copy.db");
    std::fs::write(&copy, b"something precious").unwrap();
    let err = s.backup(&copy, false).unwrap_err();
    assert!(matches!(err, Error::AlreadyExists), "{err:?}");
    assert_eq!(std::fs::read(&copy).unwrap(), b"something precious");

    s.backup(&copy, true).unwrap();
    assert_eq!(titles(&Store::open(&copy).unwrap()).len(), 3);
    // Replacing an older backup, with its leftover -wal from being opened, replaces both.
    std::fs::write(out.0.join("copy.db-wal"), b"stale").unwrap();
    s.add_task(NewTask::new("Four"), &Actor::Me).unwrap();
    s.backup(&copy, true).unwrap();
    assert!(!out.0.join("copy.db-wal").exists());
    assert_eq!(titles(&Store::open(&copy).unwrap()).len(), 4);
}

#[test]
fn the_database_itself_is_never_the_target() {
    let dir = TempDir::new();
    let s = seeded(&dir);
    let path = s.path().unwrap();
    for overwrite in [false, true] {
        let err = s.backup(&path, overwrite).unwrap_err();
        assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    }
    // Even named another way.
    let roundabout = dir.0.join(".").join("airboss.db");
    let err = s.backup(&roundabout, true).unwrap_err();
    assert!(matches!(err, Error::InvalidTarget), "{err:?}");
    assert_eq!(titles(&s).len(), 3);
}

#[test]
fn a_folder_that_does_not_exist_fails_and_leaves_nothing() {
    let dir = TempDir::new();
    let s = seeded(&dir);
    let out = TempDir::new();
    let err = s
        .backup(&out.0.join("missing").join("copy.db"), false)
        .unwrap_err();
    assert!(matches!(err, Error::Db(_)), "{err:?}");
    assert_eq!(std::fs::read_dir(&out.0).unwrap().count(), 0);
}

#[test]
fn an_in_memory_store_backs_up_too() {
    let mut s = Store::open_in_memory().unwrap();
    s.add_task(NewTask::new("Only"), &Actor::Me).unwrap();
    let out = TempDir::new();
    let copy = out.0.join("copy.db");
    s.backup(&copy, false).unwrap();
    assert_eq!(titles(&Store::open(&copy).unwrap()), vec!["Only"]);
}
