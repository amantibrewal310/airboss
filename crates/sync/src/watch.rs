//! [`KqueueWatch`]: the real watcher, `notify`'s kqueue backend.
//!
//! FSEvents doesn't report writes to a WAL file a process keeps open, so the `-wal` file
//! itself is watched with kqueue. kqueue drops a watch when its file is deleted or renamed
//! (T3 closing its last connection deletes the WAL); the next call to `watch`, which the T3
//! worker makes on every folder event, adds it again once the file exists.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use airboss_services::{ErrorCode, ServiceError};
use notify::event::{EventKind, ModifyKind};
use notify::{Config, KqueueWatcher, RecursiveMode, Watcher};

use crate::worker::{T3Msg, WalWatch, WatchTarget, log};

#[derive(Default)]
struct Shared {
    tx: Option<Sender<T3Msg>>,
    targets: HashMap<PathBuf, WatchTarget>,
    /// Paths kqueue is watching now.
    watched: HashSet<PathBuf>,
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Watches T3's folder and `-wal`, and Claude's sessions folder and files, with kqueue.
#[derive(Default)]
pub struct KqueueWatch {
    watcher: Option<KqueueWatcher>,
    shared: Arc<Mutex<Shared>>,
}

impl KqueueWatch {
    pub fn new() -> KqueueWatch {
        KqueueWatch::default()
    }
}

fn path_of(t: &WatchTarget) -> &Path {
    match t {
        WatchTarget::T3Dir(p)
        | WatchTarget::T3Wal(p)
        | WatchTarget::ClaudeDir(p)
        | WatchTarget::ClaudeFile(p) => p,
    }
}

/// What one kqueue event means for the T3 worker.
fn handle(shared: &Mutex<Shared>, event: notify::Result<notify::Event>) {
    let event = match event {
        Ok(e) => e,
        Err(e) => return log(&format!("watcher error: {e}")),
    };
    let ends_watch = matches!(
        event.kind,
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    );
    let data = matches!(event.kind, EventKind::Modify(ModifyKind::Data(_)));
    let metadata_only = matches!(event.kind, EventKind::Modify(ModifyKind::Metadata(_)))
        || matches!(event.kind, EventKind::Access(_));
    let mut s = lock(shared);
    for path in &event.paths {
        let Some(target) = s.targets.get(path).cloned() else {
            continue;
        };
        if ends_watch {
            // kqueue has dropped this watch; a later `watch` call adds it again.
            s.watched.remove(path);
        }
        let msg = match target {
            WatchTarget::T3Wal(_) if data => Some(T3Msg::Wal),
            WatchTarget::T3Dir(_) if !metadata_only => Some(T3Msg::T3Dir),
            WatchTarget::ClaudeDir(_) if !metadata_only => Some(T3Msg::ClaudeDir),
            WatchTarget::ClaudeFile(p) if data || ends_watch => Some(T3Msg::ClaudeFile(p)),
            _ => None,
        };
        if let (Some(msg), Some(tx)) = (msg, &s.tx) {
            let _ = tx.send(msg);
        }
    }
}

impl WalWatch for KqueueWatch {
    fn watch(&mut self, targets: &[WatchTarget], tx: Sender<T3Msg>) -> Result<(), ServiceError> {
        {
            let mut s = lock(&self.shared);
            s.tx = Some(tx);
            s.targets = targets
                .iter()
                .map(|t| (path_of(t).to_path_buf(), t.clone()))
                .collect();
        }
        if self.watcher.is_none() {
            let shared = self.shared.clone();
            let w = KqueueWatcher::new(move |e| handle(&shared, e), Config::default()).map_err(
                |e| ServiceError::new(ErrorCode::Internal, format!("kqueue failed: {e}")),
            )?;
            self.watcher = Some(w);
        }
        let Some(w) = self.watcher.as_mut() else {
            return Ok(());
        };
        let (wanted, watched): (HashSet<PathBuf>, HashSet<PathBuf>) = {
            let s = lock(&self.shared);
            (s.targets.keys().cloned().collect(), s.watched.clone())
        };
        for old in watched.difference(&wanted) {
            let _ = w.unwatch(old);
            lock(&self.shared).watched.remove(old);
        }
        let mut first_error = None;
        for path in wanted.difference(&watched) {
            // A target that doesn't exist yet (a WAL while T3 is closed) waits for a later call.
            if !path.exists() {
                continue;
            }
            match w.watch(path, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    lock(&self.shared).watched.insert(path.clone());
                }
                Err(e) => {
                    first_error.get_or_insert(format!("{}: {e}", path.display()));
                }
            }
        }
        match first_error {
            Some(why) => Err(ServiceError::new(ErrorCode::Internal, why)),
            None => Ok(()),
        }
    }
}
