//! How each source is doing: T3, Claude, and each `gh` account.

use std::time::{Duration, SystemTime};

/// `sync_health.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealthState {
    Ok,
    /// Not there at all (no `gh`, no T3, no Claude folder) or signed out.
    Off,
    Broken,
    RateLimited,
}

impl HealthState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            HealthState::Ok => "ok",
            HealthState::Off => "off",
            HealthState::Broken => "broken",
            HealthState::RateLimited => "rate_limited",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<HealthState> {
        Some(match text {
            "ok" => HealthState::Ok,
            "off" => HealthState::Off,
            "broken" => HealthState::Broken,
            "rate_limited" => HealthState::RateLimited,
            _ => return None,
        })
    }
}

/// One source's health, as kept in memory and in `sync_health`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Health {
    pub state: HealthState,
    /// What's wrong, in plain words; `None` when ok.
    pub detail: Option<String>,
    /// When `state` last changed.
    pub since: SystemTime,
    pub last_ok_at: Option<SystemTime>,
    /// Failures in a row; drives backoff.
    pub failures: u32,
    /// Backoff or rate-limit end.
    pub next_try_at: Option<SystemTime>,
    /// Not read since the app started: what's shown is from before.
    pub stale: bool,
}

impl Health {
    /// Not read yet.
    pub(crate) fn unknown(now: SystemTime) -> Health {
        Health {
            state: HealthState::Off,
            detail: None,
            since: now,
            last_ok_at: None,
            failures: 0,
            next_try_at: None,
            stale: true,
        }
    }

    pub(crate) fn is_ok(&self) -> bool {
        self.state == HealthState::Ok
    }

    /// Ok, and read since the app started.
    pub(crate) fn is_live(&self) -> bool {
        self.is_ok() && !self.stale
    }

    /// A good read at `now`.
    pub(crate) fn ok(&mut self, now: SystemTime) {
        if self.state != HealthState::Ok {
            self.since = now;
        }
        self.state = HealthState::Ok;
        self.detail = None;
        self.last_ok_at = Some(now);
        self.failures = 0;
        self.next_try_at = None;
        self.stale = false;
    }

    /// Moves to `state` with `detail`; `since` changes only when the state does.
    pub(crate) fn set(&mut self, state: HealthState, detail: impl Into<String>, now: SystemTime) {
        if self.state != state {
            self.since = now;
        }
        self.state = state;
        self.detail = Some(detail.into());
        self.stale = false;
        if state != HealthState::RateLimited {
            self.next_try_at = None;
        }
    }

    /// One more failure in a row: broken, with the next try after 1, 2, 4 … 30 minutes.
    pub(crate) fn fail(&mut self, detail: impl Into<String>, now: SystemTime) {
        self.set(HealthState::Broken, detail, now);
        self.failures = self.failures.saturating_add(1);
        self.next_try_at = Some(now + backoff(self.failures));
    }

    /// Rate-limited until `until`.
    pub(crate) fn limit(&mut self, detail: impl Into<String>, until: SystemTime, now: SystemTime) {
        self.set(HealthState::RateLimited, detail, now);
        self.next_try_at = Some(until);
    }

    /// Whether this differs enough from what was last saved to write it: any change but a
    /// newer `last_ok_at`, which is saved at most once a minute so a busy source doesn't
    /// write a row on every pass.
    pub(crate) fn worth_saving(&self, saved: &Health) -> bool {
        let moved_ok = match (self.last_ok_at, saved.last_ok_at) {
            (Some(now), Some(then)) => now
                .duration_since(then)
                .map_or(true, |d| d >= Duration::from_secs(60)),
            (a, b) => a != b,
        };
        (
            self.state,
            &self.detail,
            self.since,
            self.failures,
            self.next_try_at,
        ) != (
            saved.state,
            &saved.detail,
            saved.since,
            saved.failures,
            saved.next_try_at,
        ) || moved_ok
    }

    /// Whether a try is allowed now: ok, or past its backoff or rate-limit end.
    pub(crate) fn may_try(&self, now: SystemTime) -> bool {
        self.next_try_at.is_none_or(|t| t <= now)
    }
}

/// 1, 2, 4 … minutes after the first, second, third … failure in a row, at most 30.
pub(crate) fn backoff(failures: u32) -> Duration {
    let minutes = 1u64 << failures.saturating_sub(1).min(5);
    Duration::from_secs(60 * minutes.min(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_thirty_minutes() {
        let mins: Vec<u64> = (1..=8).map(|n| backoff(n).as_secs() / 60).collect();
        assert_eq!(mins, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn backoff_and_budget_changes_are_saved_and_a_fresh_ok_only_once_a_minute() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut h = Health::unknown(t0);
        h.ok(t0);
        let saved = h.clone();
        h.ok(t0 + Duration::from_secs(30));
        assert!(
            !h.worth_saving(&saved),
            "a newer good read alone, inside a minute"
        );
        h.ok(t0 + Duration::from_secs(61));
        assert!(h.worth_saving(&saved));
        let mut f = saved.clone();
        f.fail("down", t0);
        let once = f.clone();
        f.fail("down", t0);
        assert!(f.worth_saving(&once), "failures and the next try are saved");
    }

    #[test]
    fn since_moves_only_when_the_state_does() {
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(10);
        let mut h = Health::unknown(t0);
        h.fail("down", t0);
        h.fail("still down", t1);
        assert_eq!(h.since, t0);
        assert_eq!(h.failures, 2);
        assert_eq!(h.next_try_at, Some(t1 + Duration::from_secs(120)));
        h.ok(t1);
        assert_eq!((h.since, h.failures, h.next_try_at), (t1, 0, None));
    }
}
