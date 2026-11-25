//! Builds the services and wires them to the store. The only place that knows which
//! implementations run.
//!
//! Until the real `sync`, `launch` and `attention` crates land, the app runs on the fakes
//! from `crates/services`. At integration this file alone changes.

use std::sync::Arc;

use airboss_services::fakes::{FakeAttention, FakeLaunch, FakeSync};
use airboss_services::{AttentionService, LaunchService, Service, SharedStore, SyncService};

/// The three services, behind their traits.
#[derive(Clone)]
pub struct Services {
    pub sync: Arc<dyn SyncService>,
    pub launch: Arc<dyn LaunchService>,
    pub attention: Arc<dyn AttentionService>,
}

impl Services {
    /// The services the app runs with, sharing `store`.
    pub fn start(_store: &SharedStore) -> Services {
        Services {
            sync: Arc::new(FakeSync::new()),
            launch: Arc::new(FakeLaunch::new()),
            attention: Arc::new(FakeAttention::new()),
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
