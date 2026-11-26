//! The T3 and Claude worker: reads T3's database when its WAL is written, Claude's session
//! files when they change and Herdr's `hc events` stream as it arrives, and publishes the
//! thread half of the snapshot.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};

use airboss_core::{Actor, Link, LinkKind, TaskId};
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{PrRef, ServiceError, Source, ThreadKey, ThreadState, ThreadView};

use super::{Env, GhMsg, Step, StoreSummary, T3Msg, WalWatch, WatchTarget, log};
use crate::claude::{self, ClaudeSession, ClaudeSource};
use crate::db::{SyncDb, WaitingPr};
use crate::health::{Health, HealthState};
use crate::herdr::{self, HerdrFeed, HerdrSource, HerdrTable, HerdrUpdate};
use crate::hub::ThreadsPart;
use crate::t3::{T3Error, T3Query, T3Source};
use crate::threads;

/// How long a pending request may go without its turn item before T3 counts as broken.
pub const ITEM_GRACE: Duration = Duration::from_secs(10);
/// How often untracked threads and T3's projects are read (Q6 and Q8).
pub const UNTRACKED_EVERY: Duration = Duration::from_secs(60);
/// How often T3's layout and values are checked again.
pub const CHECK_EVERY: Duration = Duration::from_secs(3600);
/// How long an `hc events` stream must have run for its end to count as the first failure
/// of a new streak, rather than one more in a row.
pub const HERDR_STEADY: Duration = Duration::from_secs(60);
/// How many restarts of `hc events` may fail in a row before Herdr counts as broken.
pub const HERDR_RESTARTS: u32 = 3;

/// The wait before restarting `hc events` after its `failures`-th end in a row: 1 s,
/// doubling to 60 s.
pub fn herdr_backoff(failures: u32) -> Duration {
    let secs = 1u64 << failures.saturating_sub(1).min(6);
    Duration::from_secs(secs.min(60))
}

/// The T3 and Claude worker. See the module docs.
pub struct T3Worker {
    env: Env,
    db: SyncDb,
    t3: Box<dyn T3Source>,
    claude: Box<dyn ClaudeSource>,
    herdr: Box<dyn HerdrSource>,
    watch: Box<dyn WalWatch>,
    me: Sender<T3Msg>,
    gh: Sender<GhMsg>,

    /// Every `t3_thread`, `claude_session` and `herdr_terminal` link, of every task.
    links: Vec<Link>,
    open: HashSet<TaskId>,

    t3_open: bool,
    last_version: Option<i64>,
    /// When the last T3 pass ran, for the gap.
    last_pass: Option<SystemTime>,
    /// A pass waiting for the end of the gap (or a request's grace).
    pass_due: Option<SystemTime>,
    /// The next pass reads even if `data_version` hasn't moved.
    force: bool,
    last_untracked: Option<SystemTime>,
    last_check: Option<SystemTime>,
    itemless_since: HashMap<String, SystemTime>,
    known_errors: HashMap<String, String>,
    noted: HashMap<String, String>,

    claude_files: BTreeSet<PathBuf>,
    /// Liveness by (pid, procStart), from the last `ps`.
    claude_alive: HashMap<(u32, String), bool>,
    /// Session statuses Claude Code added that Airboss doesn't know, each logged once.
    claude_unknown: BTreeSet<String>,

    /// The current `hc events` stream's number; lines from older ones are ignored.
    herdr_stream: u64,
    /// When the current stream started; `None` while none runs.
    herdr_started: Option<SystemTime>,
    /// When to start the stream again, after it ended.
    herdr_restart_at: Option<SystemTime>,
    /// Ends in a row, each before its snapshot and within [`HERDR_STEADY`] of its start.
    herdr_failures: u32,
    /// Every terminal, from the latest snapshot on; `None` until one arrives.
    herdr_table: Option<HerdrTable>,

    part: ThreadsPart,
    /// What `observed_threads` holds, so rows are written only on change.
    stored: HashMap<ThreadKey, ThreadView>,
    /// The health last written to `sync_health`, by source.
    saved_health: HashMap<&'static str, Health>,
}

fn key(source: Source, r: &str) -> ThreadKey {
    ThreadKey {
        source,
        r#ref: r.to_string(),
    }
}

fn kind_source(kind: LinkKind) -> Option<Source> {
    match kind {
        LinkKind::T3Thread => Some(Source::T3),
        LinkKind::ClaudeSession => Some(Source::Claude),
        LinkKind::HerdrTerminal => Some(Source::Herdr),
        _ => None,
    }
}

/// A view without its `stale` flag, which only the hub sets.
fn same(a: &ThreadView, b: &ThreadView) -> bool {
    let mut a = a.clone();
    a.stale = b.stale;
    a == *b
}

impl T3Worker {
    /// A worker that hasn't read anything yet; [`start`](T3Worker::start) does the first
    /// passes. `open` is the open tasks from the one `core` snapshot read at start.
    // One argument per source it reads, plus its channels and the open tasks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        env: Env,
        t3: Box<dyn T3Source>,
        claude: Box<dyn ClaudeSource>,
        herdr: Box<dyn HerdrSource>,
        watch: Box<dyn WalWatch>,
        me: Sender<T3Msg>,
        gh: Sender<GhMsg>,
        open: HashSet<TaskId>,
    ) -> Result<T3Worker, ServiceError> {
        let db = SyncDb::open(&env.store)?;
        let mut part = ThreadsPart::default();
        let mut stored = HashMap::new();
        let mut noted = HashMap::new();
        let rows = db
            .load_threads()
            .map_err(|e| crate::db::storage(format!("sync couldn't read its threads: {e}")))?;
        for row in rows {
            if let Some(run) = &row.noted_run_id {
                noted.insert(row.view.key.r#ref.clone(), run.clone());
            }
            part.threads.insert(row.view.key.clone(), row.view.clone());
            stored.insert(row.view.key.clone(), row.view);
        }
        for (source, scope, health) in db.load_health().unwrap_or_default() {
            match (source.as_str(), scope.as_str()) {
                ("t3", "") => part.t3 = health,
                ("claude", "") => part.claude = health,
                ("herdr", "") => part.herdr = health,
                _ => {}
            }
        }
        let mut worker = T3Worker {
            env,
            db,
            t3,
            claude,
            herdr,
            watch,
            me,
            gh,
            links: Vec::new(),
            open,
            t3_open: false,
            last_version: None,
            last_pass: None,
            pass_due: None,
            force: true,
            last_untracked: None,
            last_check: None,
            itemless_since: HashMap::new(),
            known_errors: HashMap::new(),
            noted,
            claude_files: BTreeSet::new(),
            claude_alive: HashMap::new(),
            claude_unknown: BTreeSet::new(),
            herdr_stream: 0,
            herdr_started: None,
            herdr_restart_at: None,
            herdr_failures: 0,
            herdr_table: None,
            part,
            stored,
            saved_health: HashMap::new(),
        };
        worker.reload_links(&[
            LinkKind::T3Thread,
            LinkKind::ClaudeSession,
            LinkKind::HerdrTerminal,
        ]);
        worker.rebuild_tasks();
        // What was seen before shows at once, stale until the first pass.
        worker.publish();
        Ok(worker)
    }

    /// The first passes, the watches, and the thread half of the loaded snapshot.
    pub fn start(&mut self, now: SystemTime) -> Step {
        self.update_watches();
        self.env.hub.publish_threads(&self.part);
        self.claude_pass(now, true);
        self.t3_pass(now);
        self.herdr_start(now);
        self.step(true, now)
    }

    /// Stops what the worker runs outside its thread (the `hc events` child). The thread
    /// loop calls it on shutdown.
    pub fn stop(&mut self) {
        self.herdr.stop();
        self.herdr_started = None;
        self.herdr_restart_at = None;
    }

    /// Whether a Herdr snapshot has arrived since the worker started, or `hc` was found
    /// missing.
    pub fn herdr_read(&self) -> bool {
        let missing = self.part.herdr.state == HealthState::Off && !self.part.herdr.stale;
        self.herdr_table.is_some() || missing
    }

    /// One message, at `now`.
    pub fn handle(&mut self, msg: T3Msg, now: SystemTime) -> Step {
        let before = self.env.hub.snapshot();
        match msg {
            T3Msg::Wal => self.want_pass(now, false),
            T3Msg::T3Dir => {
                self.update_watches();
                self.want_pass(now, false);
            }
            T3Msg::ClaudeDir | T3Msg::ClaudeFile(_) => self.claude_pass(now, false),
            T3Msg::Store(summary) => self.follow(summary, now),
            T3Msg::Refresh => self.want_pass(now, true),
            T3Msg::FindThread {
                project,
                branch,
                reply,
            } => {
                let found = match self.t3.find_thread(&project, &branch) {
                    Ok(found) => found,
                    Err(e) => {
                        self.t3_failed(e, now);
                        None
                    }
                };
                let _ = reply.send(found);
            }
            T3Msg::Herdr { stream, feed } => {
                if stream == self.herdr_stream {
                    match feed {
                        HerdrFeed::Update(u) => self.herdr_update(u, now),
                        HerdrFeed::Ended(why) => self.herdr_ended(why, now),
                    }
                }
            }
            T3Msg::Woke => {
                self.claude_pass(now, true);
                self.want_pass(now, false);
                // A stream may not survive a sleep without noticing: read Herdr afresh.
                self.herdr_start(now);
            }
            T3Msg::Recheck => {
                if !self.part.t3.is_ok() {
                    self.t3_open = false;
                }
                self.update_watches();
                self.claude_pass(now, true);
                self.want_pass(now, true);
                self.herdr_failures = 0;
                self.herdr_start(now);
            }
            T3Msg::Tick => {
                // The GitHub timer: T3 retried if it's off or broken, `data_version` read as
                // the safety net, the hourly checks, and Claude liveness.
                if self.last_check.is_some_and(|c| now >= c + CHECK_EVERY) {
                    self.t3_open = false;
                }
                self.want_pass(now, false);
                self.claude_pass(now, true);
                // Herdr installed since it was found missing.
                if self.part.herdr.state == HealthState::Off {
                    self.herdr_start(now);
                }
            }
            T3Msg::Shutdown => {}
        }
        if self.pass_due.is_some_and(|due| due <= now) {
            self.t3_pass(now);
        }
        if self.herdr_restart_at.is_some_and(|at| at <= now) {
            self.herdr_start(now);
        }
        let changed = !std::sync::Arc::ptr_eq(&before, &self.env.hub.snapshot());
        self.step(changed, now)
    }

    fn step(&self, changed: bool, now: SystemTime) -> Step {
        let wanted = [self.pass_due, self.herdr_restart_at]
            .into_iter()
            .flatten()
            .min();
        Step::at(changed, wanted, now)
    }

    /// Marks the source broken after a panic in `handle`.
    pub fn panicked(&mut self, now: SystemTime) {
        self.t3_open = false;
        self.part.t3.set(
            HealthState::Broken,
            "sync hit an internal error reading T3",
            now,
        );
        self.publish();
    }

    // ---- following core ----

    fn reload_links(&mut self, kinds: &[LinkKind]) {
        let fresh: Vec<Link> = {
            let store = self.env.store();
            let mut out = Vec::new();
            for kind in kinds {
                match store.links_of_kind(*kind) {
                    Ok(links) => out.extend(links),
                    Err(e) => log(&format!("couldn't read {kind} links: {e}")),
                }
            }
            out
        };
        self.links.retain(|l| !kinds.contains(&l.kind));
        self.links.extend(fresh);
    }

    /// Thread ids of `source` linked to an open task, sorted, once each.
    fn open_linked(&self, source: Source) -> Vec<String> {
        let ids: BTreeSet<String> = self
            .links
            .iter()
            .filter(|l| kind_source(l.kind) == Some(source) && self.open.contains(&l.task))
            .map(|l| l.r#ref.clone())
            .collect();
        ids.into_iter().collect()
    }

    fn linked_anywhere(&self, source: Source) -> HashSet<String> {
        self.links
            .iter()
            .filter(|l| kind_source(l.kind) == Some(source))
            .map(|l| l.r#ref.clone())
            .collect()
    }

    fn rebuild_tasks(&mut self) {
        let mut tasks: HashMap<ThreadKey, Vec<TaskId>> = HashMap::new();
        for l in &self.links {
            if let Some(source) = kind_source(l.kind)
                && self.open.contains(&l.task)
            {
                tasks.entry(key(source, &l.r#ref)).or_default().push(l.task);
            }
        }
        for ids in tasks.values_mut() {
            ids.sort();
            ids.dedup();
        }
        self.part.thread_tasks = tasks;
        // Only threads on open tasks, and untracked ones, are shown.
        let tasks = &self.part.thread_tasks;
        self.part
            .threads
            .retain(|k, v| !v.linked || tasks.contains_key(k));
    }

    fn follow(&mut self, s: StoreSummary, now: SystemTime) {
        let t3_before = self.open_linked(Source::T3);
        let claude_before = self.open_linked(Source::Claude);
        let herdr_before = self.open_linked(Source::Herdr);
        let kinds: Vec<LinkKind> = s
            .kinds()
            .into_iter()
            .filter(|k| kind_source(*k).is_some())
            .collect();
        self.open = s.open;
        if !kinds.is_empty() {
            self.reload_links(&kinds);
        }
        self.rebuild_tasks();
        if self.open_linked(Source::T3) != t3_before {
            // A new thread link (or a task opening) reads at once, skipping the gap.
            self.want_pass(now, true);
        }
        if self.open_linked(Source::Claude) != claude_before {
            self.claude_pass(now, false);
        }
        if self.open_linked(Source::Herdr) != herdr_before {
            self.herdr_views(now);
        }
        self.publish();
    }

    // ---- T3 ----

    /// A pass now, or at the end of the gap. `skip_gap` also reads past `data_version`.
    fn want_pass(&mut self, now: SystemTime, skip_gap: bool) {
        if skip_gap {
            self.force = true;
        }
        let gap_end = self.last_pass.map(|p| p + self.env.cfg.t3_min_gap);
        match gap_end {
            Some(end) if !skip_gap && now < end => {
                self.pass_due = Some(self.pass_due.map_or(end, |d| d.min(end)));
            }
            _ => self.t3_pass(now),
        }
    }

    fn t3_failed(&mut self, e: T3Error, now: SystemTime) {
        match e {
            T3Error::Missing => {
                self.t3_open = false;
                self.part
                    .t3
                    .set(HealthState::Off, "T3 isn't installed", now);
                // No database: every linked thread is gone, so none can hold a finish.
                let views = self
                    .open_linked(Source::T3)
                    .iter()
                    .map(|id| {
                        let v = threads::gone_view(id);
                        (v.key.clone(), v)
                    })
                    .collect();
                if self.replace(Source::T3, views, now) {
                    self.threads_changed(now);
                }
            }
            T3Error::Busy => {
                // Skipped; the next event retries.
                self.last_version = None;
            }
            T3Error::Broken(why) => {
                self.t3_open = false;
                if self.part.t3.state != HealthState::Broken {
                    log(&format!("T3 sync broken: {why}"));
                }
                self.part.t3.set(HealthState::Broken, why, now);
            }
        }
        self.save_health("t3");
        self.publish();
    }

    fn t3_pass(&mut self, now: SystemTime) {
        self.pass_due = None;
        self.last_pass = Some(now);
        if !self.t3_open {
            if let Err(e) = self.t3.open() {
                return self.t3_failed(e, now);
            }
            self.t3_open = true;
            self.last_check = Some(now);
            self.force = true;
            self.part.environment = self.t3.environment_id().unwrap_or(None);
        }
        let version = match self.t3.data_version() {
            Ok(v) => v,
            Err(e) => return self.t3_failed(e, now),
        };
        if !self.force && Some(version) == self.last_version {
            // Nothing new: still a good read.
            self.part.t3.ok(now);
            self.save_health("t3");
            return self.publish();
        }
        self.force = false;
        self.last_version = Some(version);
        let ids = self.open_linked(Source::T3);
        let untracked_due = self
            .last_untracked
            .is_none_or(|t| now >= t + UNTRACKED_EVERY);
        let query = T3Query {
            linked: ids.clone(),
            known_failures: self.known_errors.keys().cloned().collect(),
            untracked_since: untracked_due.then(|| {
                to_ts(
                    now.checked_sub(self.env.cfg.untracked_window)
                        .unwrap_or(SystemTime::UNIX_EPOCH),
                )
            }),
        };
        let read = match self.t3.read(&query) {
            Ok(read) => read,
            Err(e) => return self.t3_failed(e, now),
        };
        let linked = threads::linked(&read, &ids, &self.known_errors);

        // A pending request with no item yet: retried, and broken after the grace.
        self.itemless_since
            .retain(|id, _| linked.itemless.contains(id));
        for id in &linked.itemless {
            let since = *self.itemless_since.entry(id.clone()).or_insert(now);
            if now >= since + ITEM_GRACE {
                return self.t3_failed(
                    T3Error::Broken(format!("T3 request {id:?} has had no turn item for 10 s")),
                    now,
                );
            }
            let due = since + ITEM_GRACE;
            self.pass_due = Some(self.pass_due.map_or(due, |d| d.min(due)));
            self.force = true;
        }

        // Only real messages are kept: a failed run whose error item isn't written yet is
        // asked about again on the next pass.
        for (run, message) in &read.failures {
            if let Some(text) = message {
                self.known_errors.insert(run.clone(), text.clone());
            }
        }
        let mut views: HashMap<ThreadKey, ThreadView> = linked
            .views
            .into_iter()
            .map(|v| (v.key.clone(), v))
            .collect();
        match &read.untracked {
            Some(rows) => {
                self.last_untracked = Some(now);
                for v in threads::untracked(rows, &self.linked_anywhere(Source::T3)) {
                    views.insert(v.key.clone(), v);
                }
            }
            None => {
                // Keep the untracked threads from the last Q6.
                for (k, v) in &self.part.threads {
                    if k.source == Source::T3 && !v.linked {
                        views.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
            }
        }
        if let Some(projects) = read.projects {
            self.part.projects = projects;
        }
        let changed_states = self.replace(Source::T3, views, now);
        self.note_failures();
        self.find_listed_prs(&read.threads);
        self.part.t3.ok(now);
        self.save_health("t3");
        if changed_states {
            self.threads_changed(now);
        }
        self.publish();
    }

    /// Replaces every view of `source` and writes the rows that changed. Returns whether
    /// any linked thread's state changed.
    fn replace(
        &mut self,
        source: Source,
        views: HashMap<ThreadKey, ThreadView>,
        now: SystemTime,
    ) -> bool {
        let mut state_changed = false;
        for (k, v) in &views {
            if v.linked
                && self
                    .part
                    .threads
                    .get(k)
                    .is_none_or(|old| old.state != v.state)
            {
                state_changed = true;
            }
            if self.stored.get(k).is_none_or(|old| !same(old, v)) {
                match self.db.save_thread(v, now) {
                    Ok(()) => {
                        self.stored.insert(k.clone(), v.clone());
                    }
                    Err(e) => log(&format!("couldn't save thread {}: {e}", k.r#ref)),
                }
            }
        }
        // Rows of this source that are neither shown now nor linked to some task go.
        let anywhere = self.linked_anywhere(source);
        let gone: Vec<ThreadKey> = self
            .stored
            .keys()
            .filter(|k| {
                k.source == source && !views.contains_key(k) && !anywhere.contains(&k.r#ref)
            })
            .cloned()
            .collect();
        for k in gone {
            if self.db.delete_thread(&k).is_ok() {
                self.stored.remove(&k);
            }
        }
        self.part.threads.retain(|k, _| k.source != source);
        self.part.threads.extend(views);
        let tasks = &self.part.thread_tasks;
        self.part
            .threads
            .retain(|k, v| !v.linked || tasks.contains_key(k));
        state_changed
    }

    /// "Run failed: …" on each open task of a linked thread whose latest run failed, once
    /// per failed run.
    fn note_failures(&mut self) {
        let failed: Vec<(ThreadKey, String, String)> = self
            .part
            .threads
            .values()
            .filter(|v| v.key.source == Source::T3 && v.linked && v.state == ThreadState::Errored)
            .filter_map(|v| Some((v.key.clone(), v.run_id.clone()?, v.error.clone()?)))
            .filter(|(k, run, _)| self.noted.get(&k.r#ref) != Some(run))
            .collect();
        for (k, run, text) in failed {
            let note = threads::failure_note(&text);
            let tasks = self.part.thread_tasks.get(&k).cloned().unwrap_or_default();
            {
                let mut store = self.env.store();
                for task in tasks {
                    if let Err(e) = store.add_note(task, &note, &Actor::SyncT3) {
                        log(&format!("couldn't add a note to task {}: {e}", task.0));
                    }
                }
            }
            if self.db.set_noted_run(&k, &run).is_ok() {
                self.noted.insert(k.r#ref.clone(), run);
            }
        }
    }

    /// PRs T3 lists on linked threads, linked at or after our thread link, go into
    /// `pr_waiting`; the GitHub worker reads them 5 s later.
    fn find_listed_prs(&mut self, read: &[crate::t3::T3Thread]) {
        let mut fresh = Vec::new();
        for t in read.iter().filter(|t| !t.gone) {
            let earliest = self
                .links
                .iter()
                .filter(|l| {
                    l.kind == LinkKind::T3Thread && l.r#ref == t.id && self.open.contains(&l.task)
                })
                .filter_map(|l| from_ts(&l.linked_at))
                .min();
            let Some(earliest) = earliest else { continue };
            for entry in &t.pull_requests {
                let (Some("github.com"), Some(repo), Some(n), Some(at)) = (
                    entry.host.as_deref(),
                    entry.repository.as_deref(),
                    entry.number,
                    entry.linked_at.as_deref(),
                ) else {
                    continue;
                };
                if from_ts(at).is_none_or(|at| at < earliest) {
                    continue;
                }
                let Some(pr) = PrRef::new(repo, n) else {
                    continue;
                };
                let waiting = WaitingPr {
                    pr: pr.as_str().to_string(),
                    url: entry.url.clone(),
                    linked_at: at.to_string(),
                };
                match self.db.add_waiting(&t.id, &waiting) {
                    Ok(true) => fresh.push(pr),
                    Ok(false) => {}
                    Err(e) => log(&format!("couldn't record a PR on thread {}: {e}", t.id)),
                }
            }
        }
        if !fresh.is_empty() {
            let _ = self.gh.send(GhMsg::Read(fresh));
        }
    }

    /// A linked thread's state changed: held finishes are checked again.
    fn threads_changed(&mut self, _now: SystemTime) {
        let _ = self.gh.send(GhMsg::Holds);
    }

    // ---- Claude ----

    fn claude_pass(&mut self, now: SystemTime, check_alive: bool) {
        let read = match self.claude.sessions() {
            Ok(read) => read,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No folder: no sessions, so every linked one is gone (and holds nothing).
                self.part
                    .claude
                    .set(HealthState::Off, "no Claude sessions folder", now);
                self.save_health("claude");
                self.claude_files.clear();
                self.update_watches();
                let views =
                    claude::views(&[], &[], &self.open_linked(Source::Claude), &HashSet::new())
                        .into_iter()
                        .map(|v| (v.key.clone(), v))
                        .collect();
                if self.replace(Source::Claude, views, now) {
                    self.threads_changed(now);
                }
                return self.publish();
            }
            Err(e) => return self.claude_broken(e.to_string(), now),
        };
        // Skipped files are watched too: Claude Code may finish one in place.
        let files: BTreeSet<PathBuf> = read
            .sessions
            .iter()
            .map(|s| s.file.clone())
            .chain(read.skipped)
            .collect();
        let sessions = read.sessions;
        let files_changed = files != self.claude_files;
        if files_changed {
            self.claude_files = files;
            self.update_watches();
        }
        let unknown = sessions.iter().any(|s| {
            !self
                .claude_alive
                .contains_key(&(s.pid, s.proc_start.clone()))
        });
        if check_alive || files_changed || unknown {
            match self.claude.alive(&sessions) {
                Ok(alive) => {
                    self.claude_alive = sessions
                        .iter()
                        .zip(alive)
                        .map(|(s, up)| ((s.pid, s.proc_start.clone()), up))
                        .collect();
                }
                Err(e) => return self.claude_broken(format!("ps failed: {e}"), now),
            }
        }
        let alive: Vec<bool> = sessions
            .iter()
            .map(|s: &ClaudeSession| {
                self.claude_alive
                    .get(&(s.pid, s.proc_start.clone()))
                    .copied()
                    .unwrap_or(false)
            })
            .collect();
        for status in claude::unknown_statuses(&sessions) {
            if self.claude_unknown.insert(status.to_string()) {
                log(&format!(
                    "Claude session status {status:?} is new to Airboss; showing it as idle"
                ));
            }
        }
        let views = claude::views(
            &sessions,
            &alive,
            &self.open_linked(Source::Claude),
            &self.linked_anywhere(Source::Claude),
        );
        let views = views.into_iter().map(|v| (v.key.clone(), v)).collect();
        let changed = self.replace(Source::Claude, views, now);
        self.part.claude.ok(now);
        self.save_health("claude");
        if changed {
            self.threads_changed(now);
        }
        self.publish();
    }

    fn claude_broken(&mut self, why: String, now: SystemTime) {
        if self.part.claude.state != HealthState::Broken {
            log(&format!("Claude sync broken: {why}"));
        }
        self.part.claude.set(HealthState::Broken, why, now);
        self.save_health("claude");
        self.publish();
    }

    // ---- Herdr ----

    /// Starts `hc events` afresh, stopping the stream that runs, if any.
    fn herdr_start(&mut self, now: SystemTime) {
        self.herdr_stream += 1;
        self.herdr_restart_at = None;
        match self.herdr.follow(self.herdr_stream, self.me.clone()) {
            Ok(()) => self.herdr_started = Some(now),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.herdr_started = None;
                self.herdr_failures = 0;
                self.herdr_table = None;
                self.part
                    .herdr
                    .set(HealthState::Off, "hc isn't installed", now);
                self.save_health("herdr");
                // No Herdr: every linked terminal is gone, so none can hold a finish.
                self.herdr_views(now);
                self.publish();
            }
            Err(e) => {
                self.herdr_started = Some(now);
                self.herdr_ended(format!("hc events couldn't start: {e}"), now);
            }
        }
    }

    fn herdr_update(&mut self, u: HerdrUpdate, now: SystemTime) {
        let snapshot = matches!(u, HerdrUpdate::Snapshot { .. });
        if !snapshot && self.herdr_table.is_none() {
            // hc always sends its snapshot first; a change before it has nothing to change.
            return;
        }
        let table = self.herdr_table.get_or_insert_with(HerdrTable::default);
        let changed = table.apply(u, now);
        if snapshot {
            if self.part.herdr.state == HealthState::Broken {
                log("Herdr sync works again");
            }
            self.part.herdr.ok(now);
            self.save_health("herdr");
            // A stream that delivered its snapshot didn't fail to restart.
            self.herdr_failures = 0;
        }
        if changed || snapshot {
            self.herdr_views(now);
        }
        self.publish();
    }

    /// The stream stopped: its child is reaped, and it starts again after the backoff.
    /// After [`HERDR_RESTARTS`] failed restarts in a row (each ending before its snapshot,
    /// and within [`HERDR_STEADY`]), Herdr is broken (and still restarted, every minute).
    fn herdr_ended(&mut self, why: String, now: SystemTime) {
        self.herdr.stop();
        let steady = self
            .herdr_started
            .is_some_and(|s| now.duration_since(s).is_ok_and(|d| d >= HERDR_STEADY));
        self.herdr_started = None;
        self.herdr_failures = if steady {
            1
        } else {
            self.herdr_failures.saturating_add(1)
        };
        self.herdr_restart_at = Some(now + herdr_backoff(self.herdr_failures));
        if self.herdr_failures > HERDR_RESTARTS {
            if self.part.herdr.state != HealthState::Broken {
                log(&format!("Herdr sync broken: {why}"));
            }
            self.part.herdr.set(HealthState::Broken, why, now);
            self.save_health("herdr");
            self.publish();
        }
    }

    /// Reports every `herdr_terminal` link on an open task: from the table, or gone when
    /// Herdr isn't installed. Until the first snapshot, what was seen before stays.
    fn herdr_views(&mut self, now: SystemTime) {
        let linked = self.open_linked(Source::Herdr);
        let views = match &self.herdr_table {
            Some(table) => herdr::views(table, &linked),
            None if self.part.herdr.state == HealthState::Off && !self.part.herdr.stale => {
                herdr::views(&HerdrTable::default(), &linked)
            }
            None => return,
        };
        let views = views.into_iter().map(|v| (v.key.clone(), v)).collect();
        if self.replace(Source::Herdr, views, now) {
            self.threads_changed(now);
        }
    }

    // ---- watching, health, publishing ----

    fn targets(&self) -> Vec<WatchTarget> {
        let cfg = &self.env.cfg;
        let mut out = vec![
            WatchTarget::T3Dir(cfg.t3_dir.clone()),
            WatchTarget::T3Wal(cfg.t3_dir.join("statev2.sqlite-wal")),
            WatchTarget::ClaudeDir(cfg.claude_sessions.clone()),
        ];
        out.extend(
            self.claude_files
                .iter()
                .cloned()
                .map(WatchTarget::ClaudeFile),
        );
        out
    }

    /// Sends the watcher the targets; it re-adds any watch that was dropped.
    fn update_watches(&mut self) {
        let targets = self.targets();
        match self.watch.watch(&targets, self.me.clone()) {
            Ok(()) => self.part.watch_error = None,
            Err(e) => {
                if self.part.watch_error.is_none() {
                    log(&format!("can't watch T3's files: {e}"));
                }
                self.part.watch_error = Some(format!(
                    "sync can't watch T3's files ({e}), so states update only every 5 minutes."
                ));
            }
        }
    }

    /// Writes a source's health when its state or reason changed.
    fn save_health(&mut self, source: &'static str) {
        let h: &Health = match source {
            "t3" => &self.part.t3,
            "herdr" => &self.part.herdr,
            _ => &self.part.claude,
        };
        if self
            .saved_health
            .get(source)
            .is_some_and(|old| !h.worth_saving(old))
        {
            return;
        }
        match self.db.save_health(source, "", h) {
            Ok(()) => {
                self.saved_health.insert(source, h.clone());
            }
            Err(e) => log(&format!("couldn't save {source} health: {e}")),
        }
    }

    fn publish(&mut self) {
        self.env.hub.publish_threads(&self.part);
    }
}
