//! The one pure function that decides what Needs me shows.

use std::sync::Arc;
use std::time::SystemTime;

use airboss_core::CoreSnapshot;
use airboss_services::{NeedsItem, NeedsKey, SyncSnapshot};

use crate::AttentionConfig;

/// Everything `compute` reads: the latest `core` and `sync` snapshots.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub core: Arc<CoreSnapshot>,
    pub sync: Arc<SyncSnapshot>,
}

/// One dismissed or snoozed item: hidden while `until` is `None` or later than now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    pub key: NeedsKey,
    pub until: Option<SystemTime>,
}

/// What `compute` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Computed {
    /// The visible items, in display order.
    pub items: Vec<NeedsItem>,
    /// Every item's key, hidden ones included.
    pub live_keys: Vec<NeedsKey>,
    /// The next moment the list could change with no new input; always later than `now`.
    pub next_wake: Option<SystemTime>,
}

/// Pure: everything attention shows follows from these.
pub fn compute(
    _input: &Inputs,
    _hidden: &[Hidden],
    _now: SystemTime,
    _cfg: &AttentionConfig,
) -> Computed {
    Computed {
        items: Vec::new(),
        live_keys: Vec::new(),
        next_wake: None,
    }
}
