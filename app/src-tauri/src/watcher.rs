//! Seeing writes from other processes: kqueue on the database's `-wal` file and its folder.
//!
//! FSEvents doesn't report writes to a WAL file that a connection keeps open, so this uses
//! `notify`'s kqueue watcher on two paths:
//! - `<db>-wal`: every commit by any process writes it.
//! - the database's folder (not recursive): an entry created or removed there re-arms the WAL
//!   watch, which covers the WAL being deleted when the last connection closes and created
//!   again, or renamed. It also notices the database file itself moving or being replaced.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use notify::{EventKind, KqueueWatcher, RecursiveMode, Watcher};
use tauri_plugin_log::log;

use crate::catchup::Trigger;

/// The running watch. Dropping it stops the kqueue thread and the re-arming thread.
pub struct Watch {
    // Held only to keep the watch alive.
    _thread: std::thread::JoinHandle<()>,
    stop: Sender<Msg>,
}

enum Msg {
    Event(notify::Result<notify::Event>),
    Stop,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.stop.send(Msg::Stop);
    }
}

impl Watch {
    /// Watches `db`'s WAL and folder, sending [`Trigger::Core`] for every change.
    /// `on_moved` is called once if the database file disappears or is replaced, and
    /// `on_stopped` if the thread gives up after a second panic.
    pub fn start(
        db: &Path,
        triggers: Sender<Trigger>,
        on_moved: impl Fn(&str) + Send + 'static,
        on_stopped: impl FnOnce() + Send + 'static,
    ) -> notify::Result<Watch> {
        let db = db.to_path_buf();
        let folder = db
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let wal = wal_path(&db);
        let inode = std::fs::metadata(&db).map(|m| m.ino()).ok();

        let (tx, rx) = mpsc::channel::<Msg>();
        let events = tx.clone();
        let mut watcher = KqueueWatcher::new(
            move |res: notify::Result<notify::Event>| {
                let _ = events.send(Msg::Event(res));
            },
            notify::Config::default(),
        )?;
        watcher.watch(&folder, RecursiveMode::NonRecursive)?;
        let mut wal_armed = arm(&mut watcher, &wal);

        let mut moved = false;
        let thread = crate::util::spawn_guarded(
            "watcher",
            move |_restarted| {
                while let Ok(Msg::Event(res)) = rx.recv() {
                    let event = match res {
                        Ok(e) => e,
                        Err(e) => {
                            log::warn!("watcher error: {e}");
                            continue;
                        }
                    };
                    let on_wal = event.paths.iter().any(|p| p == &wal);
                    if on_wal
                        && matches!(
                            event.kind,
                            EventKind::Remove(_)
                                | EventKind::Modify(notify::event::ModifyKind::Name(_))
                        )
                    {
                        // kqueue dropped the watch; the folder event re-arms it.
                        wal_armed = false;
                    }
                    if !on_wal {
                        // Something in the folder changed: re-arm the WAL watch on whatever
                        // file has that name now, and check the database is still there.
                        if wal_armed {
                            let _ = watcher.unwatch(&wal);
                        }
                        wal_armed = arm(&mut watcher, &wal);
                        let now = std::fs::metadata(&db).map(|m| m.ino()).ok();
                        if !moved && now != inode {
                            moved = true;
                            on_moved(if now.is_none() {
                                "The database file was moved or deleted."
                            } else {
                                "The database file was replaced."
                            });
                        }
                    }
                    if triggers.send(Trigger::Core).is_err() {
                        return;
                    }
                }
            },
            on_stopped,
        )
        .map_err(notify::Error::io)?;
        Ok(Watch {
            _thread: thread,
            stop: tx,
        })
    }
}

/// `airboss.db` → `airboss.db-wal`.
pub fn wal_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push("-wal");
    PathBuf::from(name)
}

/// Watches the WAL if it exists now; `true` if the watch is on.
fn arm(watcher: &mut KqueueWatcher, wal: &Path) -> bool {
    wal.exists() && watcher.watch(wal, RecursiveMode::NonRecursive).is_ok()
}
