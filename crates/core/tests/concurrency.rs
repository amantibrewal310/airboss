//! Several handles on one database file, as `app`, `mcp` and `sync` may have.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use airboss_core::{Actor, NewTask, Store};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "airboss-concurrency-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn four_handles_writing_at_once_all_succeed_with_gap_free_positions() {
    let dir = TempDir::new();
    let path = dir.0.join("airboss.db");
    Store::open(&path).unwrap();

    let writers: Vec<_> = (0..4)
        .map(|w| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                for i in 0..50 {
                    store
                        .add_task(NewTask::new(format!("writer {w} task {i}")), &Actor::Me)
                        .unwrap();
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }

    let store = Store::open(&path).unwrap();
    let positions: Vec<u32> = store
        .tree()
        .unwrap()
        .iter()
        .map(|n| n.task.position)
        .collect();
    assert_eq!(positions, (1..=200).collect::<Vec<u32>>());
}
