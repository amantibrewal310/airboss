//! macOS notifications for Needs me (SPEC-app "Notifications").
//!
//! **When** ([`Memory::decide`], pure): one notification for each *new* item of the kinds that
//! block an agent or need me now: an agent's question or approval, a failed run, an agent that
//! stopped. Never for proposals, reviews, ready tasks, or quiet work: quiet when
//! nothing's happening. Never for an item that existed at launch, never twice for one key, and
//! never while the main window is focused (the item is in front of me already) or with the
//! setting off. An item that arrives then is still remembered, so it doesn't notify later.
//!
//! **Withdrawing** ([`Memory::gone`]): when an item that may have a notification in
//! Notification Center leaves Needs me (answered, dismissed, done), that notification goes.
//!
//! **Delivery** goes through [`Notifications`], so the decisions are tested with
//! [`RecordingNotifications`] and the app posts through `UNUserNotificationCenter` (`macos.rs`).

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use airboss_core::TaskId;
use airboss_services::{NeedsItem, NeedsKey, NeedsKind, NeedsList};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The kinds that notify: an agent waiting on me, or a run that ended badly.
pub fn notifies(kind: NeedsKind) -> bool {
    matches!(
        kind,
        NeedsKind::Question | NeedsKind::Approval | NeedsKind::FailedRun | NeedsKind::AgentStopped
    )
}

/// How long after launch a list still counts as "what was there at launch" if sync never
/// finishes a first read (T3 and GitHub both off).
pub const SETTLE: Duration = Duration::from_secs(60);

/// One notification: about one item, titled by its task, grouped by its task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub key: NeedsKey,
    pub task: TaskId,
    /// "144 Web: slot picker".
    pub title: String,
    /// The item's one line, e.g. "Agent in slot-picker asks: Which calendar?".
    pub body: String,
    /// "task-144": notifications about one task sit together.
    pub group: String,
}

impl Notice {
    fn of(item: &NeedsItem) -> Notice {
        Notice {
            key: item.key.clone(),
            task: item.task,
            title: format!("{} {}", item.task.0, item.task_title),
            body: item.headline.clone(),
            group: format!("task-{}", item.task.0),
        }
    }
}

/// What can change the answer besides the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// Settings › Notifications.
    pub enabled: bool,
    /// The main window has focus.
    pub focused: bool,
    pub now: SystemTime,
}

/// What the decisions remember: every key seen, the keys that may have a notification in
/// Notification Center, and whether launch is over.
#[derive(Debug)]
pub struct Memory {
    seen: HashSet<NeedsKey>,
    delivered: HashSet<NeedsKey>,
    started: SystemTime,
    settled: bool,
}

impl Memory {
    /// For an app started at `started`.
    pub fn new(started: SystemTime) -> Memory {
        Memory {
            seen: HashSet::new(),
            delivered: HashSet::new(),
            started,
            settled: false,
        }
    }

    /// The notifications `list` calls for, remembering every key in it.
    ///
    /// While launch lasts, the lists fill with what was already waiting (sync's first read
    /// brings T3's open questions), so an item notifies then only if it began waiting after
    /// the start (`since`). Launch is over with the first list built after sync read every
    /// source (`checked_at`), or [`SETTLE`] after the start; from then on every new key counts.
    ///
    /// The time is checked before the list is judged: this runs only when the list changes,
    /// so its first change may come long after [`SETTLE`] (an Agent stopped item appears ten
    /// minutes after its thread stopped), and that list isn't launch. The list built with
    /// `checked_at` during launch still is: it holds what sync found already waiting.
    pub fn decide(&mut self, list: &NeedsList, at: Moment) -> Vec<Notice> {
        self.settled |= at.now >= self.started + SETTLE;
        let launching = !self.settled;
        self.settled |= list.checked_at.is_some();
        let mut out = Vec::new();
        for i in &list.items {
            // A stale item (its source is down) may not be real; it's judged once it's fresh.
            if i.stale || self.seen.contains(&i.key) {
                continue;
            }
            self.seen.insert(i.key.clone());
            let new = !launching || self.began_after_start(i);
            if !notifies(i.kind) {
                continue;
            }
            if new && at.enabled && !at.focused {
                out.push(Notice::of(i));
                self.delivered.insert(i.key.clone());
            } else if !new {
                // Waiting since before the start: an earlier run may have posted it.
                self.delivered.insert(i.key.clone());
            }
        }
        // Grouped by task: a task's notifications go out together, in list order.
        out.sort_by_key(|n| n.task);
        out
    }

    /// The keys whose notifications to withdraw: those that may be in Notification Center
    /// and whose items are no longer in `list`. Each is returned once.
    pub fn gone(&mut self, list: &NeedsList) -> Vec<NeedsKey> {
        let present: HashSet<&NeedsKey> = list.items.iter().map(|i| &i.key).collect();
        let mut out: Vec<NeedsKey> = self
            .delivered
            .iter()
            .filter(|k| !present.contains(k))
            .cloned()
            .collect();
        for k in &out {
            self.delivered.remove(k);
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

impl Memory {
    fn began_after_start(&self, i: &NeedsItem) -> bool {
        airboss_services::time::from_ts(&i.since).is_some_and(|t| t > self.started)
    }
}

/// Whether macOS lets Airboss notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "NotificationPermission", export_to = "types.ts")]
pub enum Permission {
    Granted,
    /// Turned off for Airboss in System Settings › Notifications.
    Denied,
    /// Not asked yet.
    NotAsked,
    /// Can't tell: a development build, outside an app bundle.
    Unknown,
}

/// The native side of notifications.
pub trait Notifications: Send + Sync {
    /// Posts one notification; clicking it opens the main window on its item. Its
    /// identifier is its key, so the same key always names the same notification.
    fn post(&self, notice: &Notice) -> Result<(), String>;
    /// Removes the notifications for these keys from Notification Center (nothing for a key
    /// that has none).
    fn withdraw(&self, keys: &[NeedsKey]) -> Result<(), String>;
    /// What macOS allows now (a short wait for its answer).
    fn permission(&self) -> Permission;
    /// Asks macOS once (it shows its own prompt); later calls answer from the setting.
    fn request(&self);
    /// Opens System Settings › Notifications at Airboss.
    fn open_settings(&self) -> Result<(), String>;
}

/// Records what would be posted: the fake for tests and the fixtures recorder.
#[derive(Debug)]
pub struct RecordingNotifications {
    posted: Mutex<Vec<Notice>>,
    withdrawn: Mutex<Vec<NeedsKey>>,
    permission: Mutex<Permission>,
    requests: Mutex<u32>,
}

impl Default for RecordingNotifications {
    fn default() -> RecordingNotifications {
        RecordingNotifications {
            posted: Mutex::new(Vec::new()),
            withdrawn: Mutex::new(Vec::new()),
            permission: Mutex::new(Permission::Granted),
            requests: Mutex::new(0),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl RecordingNotifications {
    /// Everything posted so far.
    pub fn posted(&self) -> Vec<Notice> {
        lock(&self.posted).clone()
    }

    /// Every key withdrawn so far, in order.
    pub fn withdrawn(&self) -> Vec<NeedsKey> {
        lock(&self.withdrawn).clone()
    }

    /// What `permission` answers.
    pub fn set_permission(&self, p: Permission) {
        *lock(&self.permission) = p;
    }

    /// How many times permission was asked for.
    pub fn requests(&self) -> u32 {
        *lock(&self.requests)
    }
}

impl Notifications for RecordingNotifications {
    fn post(&self, notice: &Notice) -> Result<(), String> {
        lock(&self.posted).push(notice.clone());
        Ok(())
    }
    fn withdraw(&self, keys: &[NeedsKey]) -> Result<(), String> {
        lock(&self.withdrawn).extend_from_slice(keys);
        Ok(())
    }
    fn permission(&self) -> Permission {
        *lock(&self.permission)
    }
    fn request(&self) {
        *lock(&self.requests) += 1;
    }
    fn open_settings(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use airboss_services::Target;

    use super::*;

    const NOW: &str = "2026-11-25T10:44:00.000Z";

    fn item(key: &str, kind: NeedsKind, task: i64) -> NeedsItem {
        NeedsItem {
            key: NeedsKey(key.into()),
            kind,
            group: kind.group(),
            task: TaskId(task),
            task_title: "Web: slot picker".into(),
            since: NOW.into(),
            headline: format!("{key} headline"),
            context: String::new(),
            target: Target::Task(TaskId(task)),
            proposed: vec![],
            stale: false,
        }
    }

    fn list(items: Vec<NeedsItem>, checked: bool) -> NeedsList {
        NeedsList {
            count: items.len() as u32,
            items,
            checked_at: checked.then(|| NOW.to_string()),
            version: 1,
        }
    }

    fn start() -> SystemTime {
        airboss_services::time::from_ts(NOW).unwrap()
    }

    fn away(secs: u64) -> Moment {
        Moment {
            enabled: true,
            focused: false,
            now: start() + Duration::from_secs(secs),
        }
    }

    /// Launch is over: sync has read once.
    fn settled() -> Memory {
        let mut m = Memory::new(start());
        assert!(m.decide(&list(vec![], true), away(1)).is_empty());
        m
    }

    fn keys(n: &[Notice]) -> Vec<&str> {
        n.iter().map(|n| n.key.0.as_str()).collect()
    }

    #[test]
    fn only_the_kinds_that_block_an_agent_or_ended_a_run_notify() {
        let mut m = settled();
        let all = [
            ("q", NeedsKind::Question),
            ("a", NeedsKind::Approval),
            ("f", NeedsKind::FailedRun),
            ("s", NeedsKind::AgentStopped),
            ("r", NeedsKind::Review),
            ("p", NeedsKind::Proposed),
            ("y", NeedsKind::Ready),
            ("z", NeedsKind::Quiet),
        ];
        let items = all.iter().map(|(k, kind)| item(k, *kind, 144)).collect();
        assert_eq!(
            keys(&m.decide(&list(items, true), away(5))),
            ["q", "a", "f", "s"]
        );
    }

    #[test]
    fn the_title_is_the_task_and_the_body_the_items_line() {
        let mut m = settled();
        let n = m.decide(
            &list(vec![item("q", NeedsKind::Question, 144)], true),
            away(5),
        );
        assert_eq!(
            n,
            [Notice {
                key: NeedsKey("q".into()),
                task: TaskId(144),
                title: "144 Web: slot picker".into(),
                body: "q headline".into(),
                group: "task-144".into(),
            }]
        );
    }

    #[test]
    fn nothing_that_existed_at_launch_notifies_even_once_sync_has_read() {
        let mut m = Memory::new(start());
        // attention's first list, before sync's first read: what's already waiting.
        let early = list(vec![item("q1", NeedsKind::Question, 1)], false);
        assert!(m.decide(&early, away(0)).is_empty());
        // sync's first read brings T3's waiting questions: still launch.
        let first = list(
            vec![
                item("q1", NeedsKind::Question, 1),
                item("q2", NeedsKind::Question, 2),
            ],
            true,
        );
        assert!(m.decide(&first, away(3)).is_empty());
        // After that, only what's new.
        let next = list(
            vec![
                item("q1", NeedsKind::Question, 1),
                item("q2", NeedsKind::Question, 2),
                item("q3", NeedsKind::Question, 3),
            ],
            true,
        );
        assert_eq!(keys(&m.decide(&next, away(9))), ["q3"]);
    }

    #[test]
    fn an_item_that_began_after_the_start_notifies_even_during_launch() {
        let mut m = Memory::new(start());
        let mut late = item("q2", NeedsKind::Question, 2);
        late.since = airboss_services::time::to_ts(start() + Duration::from_secs(20));
        let n = m.decide(
            &list(vec![item("q1", NeedsKind::Question, 1), late], true),
            away(25),
        );
        assert_eq!(keys(&n), ["q2"]);
    }

    #[test]
    fn launch_ends_after_a_minute_if_sync_never_reads() {
        let mut m = Memory::new(start());
        assert!(m.decide(&list(vec![], false), away(1)).is_empty());
        assert!(m.decide(&list(vec![], false), away(61)).is_empty());
        let n = m.decide(
            &list(vec![item("f", NeedsKind::FailedRun, 7)], false),
            away(70),
        );
        assert_eq!(keys(&n), ["f"]);
    }

    /// QA #23: the list changes for the first time ten minutes after the start, when a thread
    /// that stopped before the start becomes "Agent stopped". Its `since` predates the start,
    /// but launch was long over: it notifies.
    #[test]
    fn the_first_change_after_the_settle_time_is_not_launch() {
        for checked in [false, true] {
            let mut m = Memory::new(start());
            // What was waiting at launch (`AppState::new`): nothing.
            assert!(m.decide(&list(vec![], false), away(0)).is_empty());
            let mut stopped = item("stopped:1", NeedsKind::AgentStopped, 1);
            stopped.since = airboss_services::time::to_ts(start() - Duration::from_secs(90));
            let n = m.decide(&list(vec![stopped], checked), away(600));
            assert_eq!(keys(&n), ["stopped:1"], "checked_at set: {checked}");
        }
    }

    #[test]
    fn a_key_never_notifies_twice_even_after_it_leaves_and_comes_back() {
        let mut m = settled();
        let one = || list(vec![item("q", NeedsKind::Question, 1)], true);
        assert_eq!(m.decide(&one(), away(5)).len(), 1);
        assert!(m.decide(&one(), away(6)).is_empty());
        // Dismissed, then restored (or a snooze ended): the same key.
        assert!(m.decide(&list(vec![], true), away(7)).is_empty());
        assert!(m.decide(&one(), away(8)).is_empty());
    }

    #[test]
    fn none_while_the_window_is_focused_or_the_setting_is_off_and_not_later_either() {
        let mut m = settled();
        let focused = Moment {
            focused: true,
            ..away(5)
        };
        let off = Moment {
            enabled: false,
            ..away(5)
        };
        assert!(
            m.decide(
                &list(vec![item("q1", NeedsKind::Question, 1)], true),
                focused
            )
            .is_empty()
        );
        assert!(
            m.decide(&list(vec![item("q2", NeedsKind::Question, 2)], true), off)
                .is_empty()
        );
        // Focus moves away and the setting comes back on: those two stay quiet.
        let both = list(
            vec![
                item("q1", NeedsKind::Question, 1),
                item("q2", NeedsKind::Question, 2),
                item("q3", NeedsKind::Approval, 3),
            ],
            true,
        );
        assert_eq!(keys(&m.decide(&both, away(9))), ["q3"]);
    }

    fn gone(n: Vec<NeedsKey>) -> Vec<String> {
        n.into_iter().map(|k| k.0).collect()
    }

    #[test]
    fn a_posted_notification_is_withdrawn_once_when_its_item_leaves() {
        let mut m = settled();
        let q = item("q", NeedsKind::Question, 1);
        let mut s = item("s", NeedsKind::Approval, 2);
        let p = item("p", NeedsKind::Proposed, 3);
        let both = list(vec![q.clone(), s.clone(), p.clone()], true);
        assert_eq!(keys(&m.decide(&both, away(5))), ["q", "s"]);
        assert!(m.gone(&both).is_empty());
        // `s` goes stale (its source is down): still listed, so its notification stays.
        s.stale = true;
        let answered = list(vec![s.clone()], true);
        m.decide(&answered, away(6));
        assert_eq!(gone(m.gone(&answered)), ["q"]);
        assert!(m.gone(&answered).is_empty(), "only once");
        let none = list(vec![], true);
        m.decide(&none, away(7));
        assert_eq!(gone(m.gone(&none)), ["s"]);
        assert!(m.gone(&none).is_empty());
    }

    #[test]
    fn nothing_is_withdrawn_for_an_item_that_was_never_posted() {
        let mut m = settled();
        let focused = Moment {
            focused: true,
            ..away(5)
        };
        let one = list(vec![item("q", NeedsKind::Question, 1)], true);
        assert!(m.decide(&one, focused).is_empty());
        let none = list(vec![], true);
        m.decide(&none, away(6));
        assert!(m.gone(&none).is_empty());
    }

    #[test]
    fn an_item_waiting_since_before_the_start_is_withdrawn_when_it_leaves() {
        // An earlier run may have posted it; it's still in Notification Center.
        let mut m = Memory::new(start());
        let mut old = item("q", NeedsKind::Question, 1);
        old.since = airboss_services::time::to_ts(start() - Duration::from_secs(300));
        let launch = list(vec![old], true);
        assert!(m.decide(&launch, away(1)).is_empty());
        let none = list(vec![], true);
        m.decide(&none, away(9));
        assert_eq!(gone(m.gone(&none)), ["q"]);
    }

    #[test]
    fn a_stale_item_waits_until_it_is_fresh() {
        let mut m = settled();
        let mut stale = item("q", NeedsKind::Question, 1);
        stale.stale = true;
        assert!(
            m.decide(&list(vec![stale.clone()], true), away(5))
                .is_empty()
        );
        stale.stale = false;
        assert_eq!(keys(&m.decide(&list(vec![stale], true), away(6))), ["q"]);
    }

    #[test]
    fn several_at_once_come_one_per_item_grouped_by_task() {
        let mut m = settled();
        let n = m.decide(
            &list(
                vec![
                    item("a9", NeedsKind::Approval, 9),
                    item("q1", NeedsKind::Question, 1),
                    item("f9", NeedsKind::FailedRun, 9),
                ],
                true,
            ),
            away(5),
        );
        assert_eq!(keys(&n), ["q1", "a9", "f9"]);
        assert_eq!(n[1].group, n[2].group);
    }
}
