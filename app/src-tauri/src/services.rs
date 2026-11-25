//! Builds the services and wires them to the store: the only place that knows which
//! implementations run.
//!
//! Start-up order: `sync` first, then `launch` and `attention`, which both read `sync`. A
//! service that fails to start never stops the app: a [`Down`] stand-in takes its place,
//! answers with nothing, and shows its reason as a broken row in the connections footer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use airboss_core::{RepoId, TaskId};
use airboss_services::{
    AttentionService, Availability, Changes, Connection, ErrorCode, LaunchReport, LaunchService,
    Mode, NeedsItem, NeedsKey, NeedsList, PrRef, PrView, Prepared, Refresh, Service, ServiceError,
    SharedStore, StartRequest, StoreChange, SyncService, SyncSnapshot, SystemClock, ThreadKey,
    Timestamp,
};
use tauri_plugin_log::log;

/// The three services, behind their traits.
#[derive(Clone)]
pub struct Services {
    pub sync: Arc<dyn SyncService>,
    pub launch: Arc<dyn LaunchService>,
    pub attention: Arc<dyn AttentionService>,
}

impl Services {
    /// Starts the real `sync`, `launch` and `attention` on the app's store (a file: `sync`
    /// refuses an in-memory one), with `home` for their standard paths.
    pub fn start(store: &SharedStore, home: &Path) -> Services {
        let sync: Arc<dyn SyncService> = match airboss_sync::start(
            airboss_sync::SyncConfig::defaults(home),
            store.clone(),
            airboss_sync::Tools::real(&airboss_sync::SyncConfig::defaults(home)),
        ) {
            Ok(s) => s,
            Err(e) => Arc::new(Down::new(Kind::Sync, e)),
        };
        let launch_cfg = airboss_launch::LaunchConfig::defaults(home);
        let launch: Arc<dyn LaunchService> = match airboss_launch::start(
            launch_cfg.clone(),
            airboss_launch::CoreAccess::new(store.clone()),
            sync.clone(),
            airboss_launch::LaunchTools::real(&launch_cfg),
        ) {
            Ok(l) => l,
            Err(e) => Arc::new(Down::new(Kind::Launch, e)),
        };
        let attention: Arc<dyn AttentionService> = match airboss_attention::start(
            airboss_attention::AttentionConfig::default(),
            store.clone(),
            sync.clone(),
            Arc::new(SystemClock),
        ) {
            Ok(a) => a,
            Err(e) => Arc::new(Down::new(Kind::Attention, e)),
        };
        Services {
            sync,
            launch,
            attention,
        }
    }

    /// Each one as a plain [`Service`], for what every service gets told.
    pub fn all(&self) -> [&dyn Service; 3] {
        [
            self.sync.as_ref() as &dyn Service,
            self.launch.as_ref() as &dyn Service,
            self.attention.as_ref() as &dyn Service,
        ]
    }
}

/// Which service a [`Down`] stands in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Sync,
    Launch,
    Attention,
}

/// A service that couldn't start: it answers with nothing and reports why.
pub struct Down {
    kind: Kind,
    error: ServiceError,
    changes: Changes,
    snapshot: Arc<SyncSnapshot>,
    needs: Arc<NeedsList>,
}

impl Down {
    /// The stand-in for `kind`, which failed with `error` (logged).
    pub fn new(kind: Kind, error: ServiceError) -> Down {
        log::error!("{kind:?} couldn't start: {}", error.message);
        Down {
            kind,
            error,
            changes: Changes::new(),
            snapshot: Arc::default(),
            needs: Arc::default(),
        }
    }

    fn refuse<T>(&self) -> Result<T, ServiceError> {
        Err(ServiceError::new(
            self.error.code,
            format!("{} {}", self.what(), self.error.message),
        ))
    }

    fn what(&self) -> &'static str {
        match self.kind {
            Kind::Sync => "Sync didn't start:",
            Kind::Launch => "Do it and Plan it didn't start:",
            Kind::Attention => "Needs me didn't start:",
        }
    }
}

impl Service for Down {
    fn connections(&self) -> Vec<Connection> {
        let name = match self.kind {
            Kind::Sync => "GitHub",
            Kind::Launch => "T3 Code",
            Kind::Attention => "Needs me",
        };
        vec![Connection {
            name: name.into(),
            ok: false,
            state: "Didn't start".into(),
            detail: self.error.message.clone(),
            fix: Some("Restart Airboss. If it happens again, the log has the details.".into()),
        }]
    }
    fn recheck(&self) {}
    fn changes(&self) -> Changes {
        self.changes.clone()
    }
    fn store_changed(&self, _c: &StoreChange) {}
    fn woke(&self) {}
    fn shutdown(&self) {}
}

impl SyncService for Down {
    fn snapshot(&self) -> Arc<SyncSnapshot> {
        self.snapshot.clone()
    }
    fn refresh(&self, _what: Refresh) {}
    fn lookup_pr(&self, _pr: &PrRef) -> Result<PrView, ServiceError> {
        self.refuse()
    }
    fn find_thread(&self, _project: &Path, _branch: &str) -> Option<ThreadKey> {
        None
    }
    fn thread_url(&self, _key: &ThreadKey) -> Option<String> {
        None
    }
}

impl LaunchService for Down {
    fn availability(&self) -> Availability {
        Availability::Missing {
            reason: format!("{} {}", self.what(), self.error.message),
        }
    }
    fn prepare(&self, _task: TaskId, _mode: Mode) -> Result<Prepared, ServiceError> {
        self.refuse()
    }
    fn start(&self, _req: &StartRequest) -> Result<LaunchReport, ServiceError> {
        self.refuse()
    }
    fn add_to_t3(&self, _repo: RepoId) -> Result<(), ServiceError> {
        self.refuse()
    }
}

impl AttentionService for Down {
    fn needs_me(&self) -> Arc<NeedsList> {
        self.needs.clone()
    }
    fn for_task(&self, _task: TaskId) -> Vec<NeedsItem> {
        Vec::new()
    }
    fn dismiss(&self, _k: &NeedsKey) -> Result<(), ServiceError> {
        self.refuse()
    }
    fn snooze(&self, _k: &NeedsKey, _until: Option<Timestamp>) -> Result<(), ServiceError> {
        self.refuse()
    }
    fn restore(&self, _k: &NeedsKey) -> Result<(), ServiceError> {
        self.refuse()
    }
}

/// The error a store with no file gets from `sync`, for tests of the stand-in.
pub fn no_file_error() -> ServiceError {
    ServiceError::new(ErrorCode::Storage, "the database has no file.")
}

/// Where the services look for T3, Claude Code and `gh`: the home folder.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_that_fails_to_start_shows_a_broken_row_and_refuses_work() {
        let down = Down::new(Kind::Launch, no_file_error());
        let rows = down.connections();
        assert_eq!(rows[0].name, "T3 Code");
        assert!(!rows[0].ok);
        assert!(matches!(down.availability(), Availability::Missing { .. }));
        assert!(down.prepare(TaskId(1), Mode::DoIt).is_err());
        let sync = Down::new(Kind::Sync, no_file_error());
        assert!(sync.snapshot().threads.is_empty());
        assert_eq!(sync.connections()[0].name, "GitHub");
    }
}
