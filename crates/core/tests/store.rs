//! The store handle itself: its file, other connections to it, and other modules' tables.
#![allow(clippy::unwrap_used)]

mod common;

use airboss_core::Store;
use common::{TempDir, store};

#[test]
fn a_file_store_knows_its_absolute_path_and_memory_has_none() {
    let dir = TempDir::new();
    let file = dir.0.join("airboss.db");
    let s = Store::open(&file).unwrap();
    let path = s.path().unwrap();
    assert!(path.is_absolute(), "{path:?}");
    assert_eq!(path.canonicalize().unwrap(), file.canonicalize().unwrap());
    assert_eq!(store().path(), None);
}

#[test]
fn a_second_store_opened_on_the_path_sees_the_same_data() {
    let dir = TempDir::new();
    let mut s = Store::open(&dir.0.join("airboss.db")).unwrap();
    s.add_task(
        airboss_core::NewTask::new("Shared"),
        &airboss_core::Actor::Me,
    )
    .unwrap();
    let other = Store::open(&s.path().unwrap()).unwrap();
    assert_eq!(other.tree().unwrap().len(), 1);
}
