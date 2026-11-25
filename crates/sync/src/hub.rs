//! What the workers publish: the snapshot, the connection rows and the change signal.
//!
//! The T3 worker owns [`ThreadsPart`], the GitHub worker [`PrsPart`]. Publishing a part
//! rebuilds the snapshot, and bumps [`Changes`] only when the snapshot changed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use airboss_core::TaskId;
use airboss_services::time::to_ts;
use airboss_services::{
    Changes, Connection, PrRef, PrView, Source, SyncSnapshot, T3Project, ThreadKey, ThreadView,
};

use crate::health::{Health, HealthState};

/// The thread half of the snapshot, and the health behind it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadsPart {
    pub threads: HashMap<ThreadKey, ThreadView>,
    pub thread_tasks: HashMap<ThreadKey, Vec<TaskId>>,
    pub projects: Vec<T3Project>,
    pub t3: Health,
    pub claude: Health,
    /// T3's environment id, for `thread_url`.
    pub environment: Option<String>,
    /// Why T3's files can't be watched, if they can't.
    pub watch_error: Option<String>,
}

impl Default for ThreadsPart {
    fn default() -> ThreadsPart {
        ThreadsPart {
            threads: HashMap::new(),
            thread_tasks: HashMap::new(),
            projects: Vec::new(),
            t3: Health::unknown(UNIX_EPOCH),
            claude: Health::unknown(UNIX_EPOCH),
            environment: None,
            watch_error: None,
        }
    }
}

/// The PR half of the snapshot, and the health behind it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrsPart {
    pub prs: HashMap<PrRef, PrView>,
    pub pr_tasks: HashMap<PrRef, Vec<TaskId>>,
    /// `gh` itself: off when missing or signed out, ok otherwise.
    pub gh: Health,
    /// Each account, active first.
    pub accounts: Vec<(String, Health)>,
}

impl Default for PrsPart {
    fn default() -> PrsPart {
        PrsPart {
            prs: HashMap::new(),
            pr_tasks: HashMap::new(),
            gh: Health::unknown(UNIX_EPOCH),
            accounts: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Parts {
    threads: ThreadsPart,
    prs: PrsPart,
}

/// Shared by both workers and the `Sync` handle.
#[derive(Default)]
pub struct Hub {
    parts: Mutex<Parts>,
    snapshot: Mutex<Arc<SyncSnapshot>>,
    changes: Changes,
    wakeups: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// "14:02 UTC": `sync` has no time-zone data, so it says which zone it means.
pub(crate) fn clock_time(t: SystemTime) -> String {
    let ts = to_ts(t);
    format!("{} UTC", ts.get(11..16).unwrap_or("?"))
}

impl Hub {
    pub fn new() -> Arc<Hub> {
        Arc::new(Hub::default())
    }

    /// The current snapshot; no I/O.
    pub fn snapshot(&self) -> Arc<SyncSnapshot> {
        lock(&self.snapshot).clone()
    }

    pub fn changes(&self) -> Changes {
        self.changes.clone()
    }

    /// How many times the worker threads have woken, for the idle measurement.
    pub fn wakeups(&self) -> u64 {
        self.wakeups.load(Ordering::Relaxed)
    }

    pub(crate) fn woke_up(&self) {
        self.wakeups.fetch_add(1, Ordering::Relaxed);
    }

    /// T3's environment id, for `thread_url`.
    pub fn environment(&self) -> Option<String> {
        lock(&self.parts).threads.environment.clone()
    }

    pub(crate) fn publish_threads(&self, part: &ThreadsPart) -> bool {
        let mut parts = lock(&self.parts);
        if parts.threads == *part {
            return false;
        }
        parts.threads = part.clone();
        self.rebuild(&parts)
    }

    pub(crate) fn publish_prs(&self, part: &PrsPart) -> bool {
        let mut parts = lock(&self.parts);
        if parts.prs == *part {
            return false;
        }
        parts.prs = part.clone();
        self.rebuild(&parts)
    }

    /// The thread half as last published, e.g. for the finish check's thread states.
    pub(crate) fn thread_states(&self) -> HashMap<ThreadKey, ThreadView> {
        lock(&self.parts).threads.threads.clone()
    }

    fn rebuild(&self, parts: &Parts) -> bool {
        let t = &parts.threads;
        let p = &parts.prs;
        let source_ok = |s: Source| match s {
            Source::T3 => t.t3.is_ok(),
            Source::Claude => t.claude.is_ok(),
        };
        let threads = t
            .threads
            .iter()
            .map(|(k, v)| {
                let mut v = v.clone();
                v.stale = !source_ok(k.source);
                (k.clone(), v)
            })
            .collect();
        // The GitHub worker marks each PR whose own account isn't ok; all are stale when
        // `gh` itself is off.
        let prs = p
            .prs
            .iter()
            .map(|(k, v)| {
                let mut v = v.clone();
                v.stale = v.stale || !p.gh.is_ok();
                (k.clone(), v)
            })
            .collect();
        let mut oks: Vec<SystemTime> = Vec::new();
        for h in [&t.t3, &t.claude] {
            if h.is_ok() {
                oks.extend(h.last_ok_at);
            }
        }
        if p.gh.is_ok() {
            for (_, h) in &p.accounts {
                if h.is_ok() {
                    oks.extend(h.last_ok_at);
                }
            }
        }
        let snapshot = SyncSnapshot {
            threads,
            prs,
            thread_tasks: t.thread_tasks.clone(),
            pr_tasks: p.pr_tasks.clone(),
            projects: t.projects.clone(),
            checked_at: oks.into_iter().min().map(to_ts),
        };
        let mut current = lock(&self.snapshot);
        if **current == snapshot {
            return false;
        }
        *current = Arc::new(snapshot);
        drop(current);
        self.changes.bump();
        true
    }

    /// The connection rows (SPEC-sync, "Trait methods").
    pub fn connections(&self) -> Vec<Connection> {
        let parts = lock(&self.parts);
        let mut rows = vec![github_row(&parts.prs), t3_row(&parts.threads)];
        if parts.threads.claude.state == HealthState::Broken {
            rows.push(Connection {
                name: "Agents".into(),
                ok: false,
                state: "Sessions can't be read".into(),
                detail: "Shows Claude Code sessions run outside T3.".into(),
                fix: parts.threads.claude.detail.clone(),
            });
        }
        rows
    }
}

const GITHUB_DETAIL: &str = "Pull requests sync every 5 minutes. A merged PR finishes its task.";
const T3_DETAIL: &str = "Shows which agents are working or waiting on you.";

fn github_row(p: &PrsPart) -> Connection {
    let row = |ok: bool, state: &str, fix: Option<String>| Connection {
        name: "GitHub".into(),
        ok,
        state: state.into(),
        detail: GITHUB_DETAIL.into(),
        fix,
    };
    if p.gh.state == HealthState::Off {
        return match p.gh.detail.as_deref() {
            Some(d) if d.contains("Install") => row(false, "Not installed", Some(d.into())),
            Some(d) => row(false, "Not signed in", Some(d.into())),
            None => row(false, "Checking", None),
        };
    }
    if let Some((login, h)) = p.accounts.iter().find(|(_, h)| !h.is_ok()) {
        let state = h.detail.clone().unwrap_or_else(|| "Not working".into());
        let fix = match h.state {
            HealthState::RateLimited => format!(
                "{login} is rate-limited by GitHub until {}; its PRs wait until then.",
                h.next_try_at.map(clock_time).unwrap_or_default()
            ),
            _ if state.contains("sign") => {
                format!("{login} needs `gh auth login` again. Run `gh auth login` in a terminal.")
            }
            _ => format!("{login}'s PRs are tried again in a few minutes."),
        };
        return row(false, &format!("{login}: {state}"), Some(fix));
    }
    row(true, "Signed in", None)
}

fn t3_row(t: &ThreadsPart) -> Connection {
    let row = |ok: bool, state: String, fix: Option<String>| Connection {
        name: "T3 Code".into(),
        ok,
        state,
        detail: T3_DETAIL.into(),
        fix,
    };
    match t.t3.state {
        HealthState::Off => row(
            false,
            "Not installed".into(),
            Some("Install T3 Code to see which agents are working.".into()),
        ),
        HealthState::Broken | HealthState::RateLimited => row(
            false,
            format!("Live state broken since {}", clock_time(t.t3.since)),
            t.t3.detail.clone(),
        ),
        HealthState::Ok => match (&t.watch_error, &t.environment) {
            (Some(why), _) => row(false, "Not live".into(), Some(why.clone())),
            (None, None) => row(
                false,
                "Can't open threads".into(),
                Some(
                    "T3's environment-id file is missing or isn't a UUID, so Open thread can't \
                     open T3. Opening T3 once writes it again."
                        .into(),
                ),
            ),
            (None, Some(_)) => row(true, "Live state".into(), None),
        },
    }
}
