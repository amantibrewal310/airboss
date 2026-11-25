//! The change signal every service gives the app.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// A change counter: [`bump`](Changes::bump) adds one and wakes every waiter.
///
/// A listener keeps the last count it saw and calls [`wait_past`](Changes::wait_past) with it,
/// so a bump between two waits is never lost: the count has already moved past `seen`.
/// Clones share one counter.
#[derive(Debug, Clone, Default)]
pub struct Changes(Arc<(Mutex<u64>, Condvar)>);

impl Changes {
    /// A counter at 0.
    pub fn new() -> Changes {
        Changes::default()
    }

    /// Adds one and wakes every waiter.
    pub fn bump(&self) {
        let (_, wake) = &*self.0;
        let mut count = self.count();
        *count = count.saturating_add(1);
        wake.notify_all();
    }

    /// The count now.
    pub fn current(&self) -> u64 {
        *self.count()
    }

    /// Returns the count as soon as it's past `seen`, at once if it already is, or after
    /// `timeout` with the count unchanged.
    pub fn wait_past(&self, seen: u64, timeout: Duration) -> u64 {
        let (_, wake) = &*self.0;
        let (count, _) = wake
            .wait_timeout_while(self.count(), timeout, |count| *count <= seen)
            .unwrap_or_else(PoisonError::into_inner);
        *count
    }

    /// The counter, even if a holder panicked: a `u64` can't be left half-written.
    fn count(&self) -> MutexGuard<'_, u64> {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
