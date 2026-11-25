//! `airboss-attention`: what's waiting on me.
//!
//! It turns the latest `core` snapshot and `sync` snapshot into the Needs me list: the list at
//! the top of the main window, the callouts in a task's panel and the menu-bar count. It
//! changes no task. Everything it shows follows from one pure function, [`compute`]. See
//! `SPEC-attention.md` at the repo root.

// Until every kind is in, some helpers are unused.
#[allow(dead_code)]
mod compute;
mod kinds;
mod quiet;

use std::time::Duration;

pub use compute::{Computed, Hidden, Inputs, compute};

/// Thresholds and limits. The defaults are the EM's (25 Nov 2026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionConfig {
    /// Work with no activity this long is Quiet: 5 days.
    pub quiet_after: Duration,
    /// How long "Snooze" hides an item when no end is given: 3 days.
    pub snooze_for: Duration,
    /// How long a task that became ready stays in Ready: 7 days.
    pub ready_window: Duration,
    /// Items the menu-bar dropdown shows: 6.
    pub menu_items: usize,
    /// Changes this close together make one recompute: 150 ms.
    pub settle: Duration,
}

const DAY: Duration = Duration::from_secs(86_400);

impl Default for AttentionConfig {
    fn default() -> AttentionConfig {
        AttentionConfig {
            quiet_after: DAY * 5,
            snooze_for: DAY * 3,
            ready_window: DAY * 7,
            menu_items: 6,
            settle: Duration::from_millis(150),
        }
    }
}
