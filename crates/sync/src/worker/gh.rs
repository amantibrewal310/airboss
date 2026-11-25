//! The GitHub worker: reads PRs with every signed-in `gh` account, records GitHub's state in
//! `core` as `sync:gh`, finishes tasks whose PRs merged, links PRs T3 found on threads, and
//! publishes the PR half of the snapshot.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use airboss_core::{Actor, Link, LinkKind, NewLink, PrState, TaskId};
use airboss_services::time::from_ts;
use airboss_services::{PrRef, Refresh, ServiceError};

use super::{Env, GhMsg, Step, StoreSummary, T3Msg, log};
use crate::db::{PrError, PrRow, SyncDb, WaitingPr};
use crate::finish;
use crate::gh::{
    Account, BATCH, GhClient, GhOff, GhOutcome, LOW_BUDGET, PrData, PrResult, SECONDARY_WAIT,
};
use crate::health::{Health, HealthState};
use crate::hub::PrsPart;
use crate::prs;

/// The first pass waits this long after start.
pub const FIRST_PASS: Duration = Duration::from_secs(3);
/// A new PR link, or one T3 found, is read this long after it appears.
pub const SOON: Duration = Duration::from_secs(5);
/// `refresh(Prs)` and `refresh(All)` run at most this often.
pub const REFRESH_GAP: Duration = Duration::from_secs(60);
/// `refresh(PrsOf(task))` runs at most this often per task.
pub const TASK_REFRESH_GAP: Duration = Duration::from_secs(10);
/// Accounts are read again this often.
pub const ACCOUNTS_EVERY: Duration = Duration::from_secs(3600);
/// PRs of tasks finished this recently stay in `pr_tasks`, for attention's Ready items.
pub const RECENT_DONE: Duration = Duration::from_secs(7 * 24 * 3600);

pub(crate) fn lock_client(c: &Mutex<GhClient>) -> MutexGuard<'_, GhClient> {
    c.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The GitHub worker. See the module docs.
pub struct GhWorker {
    env: Env,
    db: SyncDb,
    client: Arc<Mutex<GhClient>>,
    t3: Sender<T3Msg>,

    /// Every PR link, of every task.
    links: Vec<Link>,
    open: HashSet<TaskId>,
    done: HashMap<TaskId, String>,
    /// `observed_prs`, as written.
    rows: HashMap<String, PrRow>,

    timer_at: SystemTime,
    soon_at: Option<SystemTime>,
    last_refresh: Option<SystemTime>,
    task_refresh: HashMap<TaskId, SystemTime>,
    /// Tasks whose PRs are all read on the next pass, merged and closed ones included.
    forced: HashSet<TaskId>,
    reread_accounts: bool,
    /// Backoff is skipped once ("Check again").
    skip_backoff: bool,

    part: PrsPart,
    saved_health: HashMap<String, (HealthState, Option<String>)>,
}

/// The `sync_health` scope for `gh` itself.
const GH_SCOPE: &str = "";

impl GhWorker {
    /// `summary` is what the one `core` snapshot read at start says.
    pub fn new(
        env: Env,
        client: Arc<Mutex<GhClient>>,
        t3: Sender<T3Msg>,
        summary: StoreSummary,
        now: SystemTime,
    ) -> Result<GhWorker, ServiceError> {
        let db = SyncDb::open(&env.store)?;
        let rows: HashMap<String, PrRow> = db
            .load_prs()
            .map_err(|e| crate::db::storage(format!("sync couldn't read its PRs: {e}")))?
            .into_iter()
            .map(|r| (r.pr.clone(), r))
            .collect();
        let mut part = PrsPart::default();
        for (source, scope, health) in db.load_health().unwrap_or_default() {
            if source == "gh" {
                if scope == GH_SCOPE {
                    part.gh = health;
                } else {
                    part.accounts.push((scope, health));
                }
            }
        }
        let mut w = GhWorker {
            env,
            db,
            client,
            t3,
            links: Vec::new(),
            open: summary.open,
            done: summary.done,
            rows,
            timer_at: now + FIRST_PASS,
            soon_at: None,
            last_refresh: None,
            task_refresh: HashMap::new(),
            forced: HashSet::new(),
            reread_accounts: true,
            skip_backoff: false,
            part,
            saved_health: HashMap::new(),
        };
        w.reload_links();
        // What was seen before shows at once, stale until the first pass.
        w.publish(now);
        Ok(w)
    }

    /// Publishes what was loaded and checks held finishes; the first pass is 3 s later.
    pub fn start(&mut self, now: SystemTime) -> Step {
        finish::recheck_held(&self.env.store, &self.db, now);
        self.publish(now);
        self.step(true, now)
    }

    pub fn handle(&mut self, msg: GhMsg, now: SystemTime) -> Step {
        let before = self.env.hub.snapshot();
        match msg {
            GhMsg::Store(s) => self.follow(s, now),
            GhMsg::Refresh(Refresh::T3) => {}
            GhMsg::Refresh(Refresh::Prs | Refresh::All) => {
                let at = self
                    .last_refresh
                    .map_or(now, |l| (l + REFRESH_GAP).max(now));
                self.last_refresh = Some(at);
                self.soon(at);
            }
            GhMsg::Refresh(Refresh::PrsOf(task)) => {
                let at = self
                    .task_refresh
                    .get(&task)
                    .map_or(now, |l| (*l + TASK_REFRESH_GAP).max(now));
                self.task_refresh.insert(task, at);
                self.forced.insert(task);
                self.soon(at);
            }
            GhMsg::Read(_) => self.soon(now + SOON),
            GhMsg::Recheck => {
                self.reread_accounts = true;
                self.skip_backoff = true;
                self.soon(now);
            }
            GhMsg::Woke | GhMsg::Tick | GhMsg::Shutdown => {}
        }
        let timer_due = self.timer_at <= now;
        if timer_due || self.soon_at.is_some_and(|s| s <= now) {
            self.pass(now);
            if timer_due {
                // The GitHub timer drives the T3 worker's periodic checks too.
                let _ = self.t3.send(T3Msg::Tick);
            }
        }
        let changed = !Arc::ptr_eq(&before, &self.env.hub.snapshot());
        self.step(changed, now)
    }

    fn soon(&mut self, at: SystemTime) {
        self.soon_at = Some(self.soon_at.map_or(at, |s| s.min(at)));
    }

    fn step(&self, changed: bool, now: SystemTime) -> Step {
        let wanted = self.soon_at.map_or(self.timer_at, |s| s.min(self.timer_at));
        Step::at(changed, Some(wanted), now)
    }

    /// Marks GitHub broken after a panic in `handle`.
    pub fn panicked(&mut self, now: SystemTime) {
        self.part
            .gh
            .fail("sync hit an internal error reading GitHub", now);
        self.timer_at = now + self.env.cfg.pr_every;
        self.soon_at = None;
        self.publish(now);
    }

    // ---- following core ----

    fn reload_links(&mut self) {
        match self.env.store().links_of_kind(LinkKind::Pr) {
            Ok(links) => self.links = links,
            Err(e) => log(&format!("couldn't read PR links: {e}")),
        }
    }

    fn follow(&mut self, s: StoreSummary, now: SystemTime) {
        let before: BTreeSet<(TaskId, String)> = self
            .open_links()
            .map(|l| (l.task, l.r#ref.clone()))
            .collect();
        let prs_changed = s.kinds().contains(&LinkKind::Pr);
        self.open = s.open;
        self.done = s.done;
        if prs_changed {
            self.reload_links();
        }
        let after: BTreeSet<(TaskId, String)> = self
            .open_links()
            .map(|l| (l.task, l.r#ref.clone()))
            .collect();
        if after.difference(&before).next().is_some() {
            self.soon(now + SOON);
        }
        // A task that left the open statuses keeps no hold.
        if let Ok(held) = self.db.holds() {
            for task in held.into_iter().filter(|t| !self.open.contains(t)) {
                let _ = self.db.release(task);
            }
        }
        self.publish(now);
    }

    fn open_links(&self) -> impl Iterator<Item = &Link> {
        self.links.iter().filter(|l| self.open.contains(&l.task))
    }

    // ---- one pass ----

    fn pass(&mut self, now: SystemTime) {
        self.soon_at = None;
        self.timer_at = now + self.env.cfg.pr_every;
        let client = self.client.clone();
        let mut client = lock_client(&client);
        let accounts_due = client.read_at().is_none_or(|t| now >= t + ACCOUNTS_EVERY);
        if self.reread_accounts || accounts_due || client.accounts().is_empty() {
            self.reread_accounts = false;
            if let Err(off) = client.read_accounts(now) {
                drop(client);
                return self.gh_off(off, now);
            }
        }
        self.part.gh.ok(now);
        self.save_health(GH_SCOPE, now);
        let accounts: Vec<Account> = client.accounts().to_vec();
        self.sync_accounts(&accounts, now);

        let waiting = self.db.waiting().unwrap_or_else(|e| {
            log(&format!("couldn't read waiting PRs: {e}"));
            Vec::new()
        });
        let wanted = self.wanted(&waiting);
        let skip_backoff = std::mem::take(&mut self.skip_backoff);
        let usable = |w: &GhWorker, login: &str| {
            accounts.iter().any(|a| a.login == login && a.ok)
                && (skip_backoff || w.health(login).is_none_or(|h| h.may_try(now)))
        };

        // Each PR goes to the account that read it last, else the first usable one. An
        // account backing off holds only its own PRs.
        let mut queue: BTreeMap<usize, Vec<PrRef>> = BTreeMap::new();
        let mut tried: HashMap<PrRef, HashSet<usize>> = HashMap::new();
        for pr in &wanted {
            let saved = self
                .rows
                .get(pr.as_str())
                .and_then(|r| r.account.as_deref())
                .and_then(|l| accounts.iter().position(|a| a.login == l && a.ok));
            let pick = match saved {
                Some(i) => usable(self, &accounts[i].login).then_some(i),
                None => accounts.iter().position(|a| usable(self, &a.login)),
            };
            if let Some(i) = pick {
                queue.entry(i).or_default().push(pr.clone());
                tried.entry(pr.clone()).or_default().insert(i);
            }
        }

        let mut found: Vec<(PrData, String)> = Vec::new();
        let mut failed: Vec<(PrRef, PrError, Option<String>, Option<String>)> = Vec::new();
        let mut viewer_ok: HashMap<String, HashSet<String>> = HashMap::new();
        let mut viewer_done: HashSet<usize> = HashSet::new();
        loop {
            // The first usable account with PRs to read, or whose own PRs aren't read yet.
            let next = (0..accounts.len()).find(|i| {
                usable(self, &accounts[*i].login)
                    && (queue.get(i).is_some_and(|q| !q.is_empty()) || !viewer_done.contains(i))
            });
            let Some(i) = next else { break };
            let login = accounts[i].login.clone();
            let pending = queue.entry(i).or_default();
            let batch: Vec<PrRef> = pending.drain(..pending.len().min(BATCH)).collect();
            let viewer = viewer_done.insert(i);
            match client.read(&login, &batch, viewer) {
                GhOutcome::Data {
                    budget,
                    viewer: mine,
                    results,
                } => {
                    self.account_mut(&login, now).ok(now);
                    if let Some(mine) = mine {
                        viewer_ok
                            .entry(login.clone())
                            .or_default()
                            .extend(mine.iter().map(|d| d.pr.as_str().to_string()));
                        found.extend(mine.into_iter().map(|d| (d, login.clone())));
                    }
                    for (pr, result) in results {
                        match result {
                            PrResult::Found(d) => found.push((d, login.clone())),
                            PrResult::NoAccess => {
                                let seen = tried.entry(pr.clone()).or_default();
                                let other = (0..accounts.len()).find(|j| {
                                    !seen.contains(j) && usable(self, &accounts[*j].login)
                                });
                                match other {
                                    Some(j) => {
                                        seen.insert(j);
                                        queue.entry(j).or_default().push(pr);
                                    }
                                    None => failed.push((pr, PrError::NoAccess, None, None)),
                                }
                            }
                            PrResult::NotFound => {
                                failed.push((pr, PrError::NotFound, None, Some(login.clone())))
                            }
                            PrResult::Failed(why) => {
                                failed.push((pr, PrError::Failed, Some(why), Some(login.clone())))
                            }
                        }
                    }
                    if let Some(b) = budget
                        && b.remaining < LOW_BUDGET
                    {
                        let until = b
                            .reset_at
                            .unwrap_or(now + SECONDARY_WAIT)
                            .max(now + Duration::from_secs(60));
                        self.account_mut(&login, now)
                            .limit("Rate-limited by GitHub", until, now);
                    }
                }
                GhOutcome::SignedOut => {
                    client.forget_token(&login);
                    self.reread_accounts = true;
                    self.account_mut(&login, now)
                        .set(HealthState::Broken, "Signed out", now);
                }
                GhOutcome::SecondaryLimit => {
                    self.account_mut(&login, now).limit(
                        "Rate-limited by GitHub",
                        now + SECONDARY_WAIT,
                        now,
                    );
                }
                GhOutcome::Failed(why) => {
                    self.account_mut(&login, now)
                        .fail(format!("Can't reach GitHub: {why}"), now);
                    for pr in &batch {
                        failed.push((
                            pr.clone(),
                            PrError::Failed,
                            Some(why.clone()),
                            Some(login.clone()),
                        ));
                    }
                }
            }
            if !self.health(&login).is_some_and(Health::is_ok) {
                // Its other PRs wait for the next pass.
                queue.remove(&i);
            }
            self.save_health_of(&login, now);
        }
        drop(client);
        self.apply(found, failed, &waiting, &viewer_ok, now);
        for task in self.forced.drain() {
            self.task_refresh.entry(task).or_insert(now);
        }
        self.publish(now);
    }

    /// The PRs to read this pass: links on open tasks GitHub hasn't settled (open, unread,
    /// failed, or where `core` disagrees), every PR of a forced task, and PRs waiting from T3.
    fn wanted(&self, waiting: &[(String, WaitingPr)]) -> Vec<PrRef> {
        let mut out: BTreeSet<PrRef> = BTreeSet::new();
        for l in &self.links {
            let row = self.rows.get(&l.r#ref);
            let unsettled = match row.and_then(|r| r.state) {
                None | Some(PrState::Open) => true,
                Some(s) => l.pr_state != Some(s),
            };
            if (self.open.contains(&l.task) && unsettled) || self.forced.contains(&l.task) {
                out.extend(PrRef::parse(&l.r#ref));
            }
        }
        out.extend(waiting.iter().filter_map(|(_, w)| PrRef::parse(&w.pr)));
        out.into_iter().collect()
    }

    /// What a pass read becomes: core writes first (PR states, links from T3, finishing),
    /// then our own rows, so a pass cut short repeats its triggers next time.
    fn apply(
        &mut self,
        found: Vec<(PrData, String)>,
        failed: Vec<(PrRef, PrError, Option<String>, Option<String>)>,
        waiting: &[(String, WaitingPr)],
        viewer_ok: &HashMap<String, HashSet<String>>,
        now: SystemTime,
    ) {
        // A PR read directly wins over the same PR in a viewer list.
        let mut reads: BTreeMap<String, (PrData, String)> = BTreeMap::new();
        for (d, login) in found {
            reads.insert(d.pr.as_str().to_string(), (d, login));
        }
        let thread_links: Vec<Link> = if waiting.is_empty() {
            Vec::new()
        } else {
            self.env
                .store()
                .links_of_kind(LinkKind::T3Thread)
                .unwrap_or_default()
        };
        let mut check: BTreeSet<TaskId> = BTreeSet::new();
        let mut linked_now: HashSet<String> = HashSet::new();
        let mut wrote_core = false;
        let mut new_rows: Vec<PrRow> = Vec::new();
        for (r, (d, login)) in &reads {
            let gh = d.state;
            let old = self.rows.get(r);
            let links: Vec<&Link> = self.links.iter().filter(|l| &l.r#ref == r).collect();
            let was_open = old.is_some_and(|o| o.state == Some(PrState::Open))
                || links.iter().any(|l| l.pr_state == Some(PrState::Open));
            let first_read = old.is_none_or(|o| o.state.is_none());
            if links.iter().any(|l| l.pr_state != Some(gh)) {
                wrote_core = true;
                // `core` reopens done tasks itself when a PR reads open again (rule 7).
                if let Err(e) = self.env.store().set_pr_state(r, gh, &Actor::SyncGh) {
                    log(&format!("couldn't record PR {r} as {gh}: {e}"));
                }
            }
            if gh == PrState::Open {
                for l in &links {
                    let _ = self.db.release(l.task);
                }
            } else if was_open || first_read {
                check.extend(
                    links
                        .iter()
                        .filter(|l| self.open.contains(&l.task))
                        .map(|l| l.task),
                );
            }
            // PRs T3 listed on a thread: linked once GitHub has read them, with its state.
            for (thread, w) in waiting.iter().filter(|(_, w)| &w.pr == r) {
                let at = from_ts(&w.linked_at);
                let tasks: Vec<TaskId> = thread_links
                    .iter()
                    .filter(|l| &l.r#ref == thread && self.open.contains(&l.task))
                    .filter(|l| {
                        from_ts(&l.linked_at)
                            .zip(at)
                            .is_some_and(|(ours, theirs)| ours <= theirs)
                    })
                    .map(|l| l.task)
                    .collect();
                for task in tasks {
                    let mut new = NewLink::pr(r.as_str()).with_pr_state(gh);
                    if let Some(url) = d.url.clone().or_else(|| w.url.clone()) {
                        new = new.with_url(url);
                    }
                    if let Some(title) = &d.title {
                        new = new.with_title(title.clone());
                    }
                    match self.env.store().link(task, new, &Actor::SyncT3) {
                        Ok(_) => {
                            linked_now.insert(r.clone());
                            if gh != PrState::Open {
                                check.insert(task);
                            }
                        }
                        Err(e) => log(&format!("couldn't link PR {r} to task {}: {e}", task.0)),
                    }
                }
                if let Err(e) = self.db.waiting_to_seen(thread, r) {
                    log(&format!(
                        "couldn't mark PR {r} seen on thread {thread}: {e}"
                    ));
                }
            }
            let linked = !links.is_empty() || linked_now.contains(r);
            new_rows.push(prs::row_from(d, old, login, linked, now));
        }
        if wrote_core || !linked_now.is_empty() {
            // Our own writes changed the links; don't wait for core's change notice.
            self.reload_links();
        }
        let fresh: HashMap<String, PrState> = reads
            .iter()
            .map(|(r, (d, _))| (r.clone(), d.state))
            .collect();
        let finished = !check.is_empty();
        for task in check {
            finish::check(&self.env.store, &self.db, task, &fresh, now);
        }
        if finished {
            self.reload_links();
        }
        for (pr, error, detail, login) in failed {
            let r = pr.as_str();
            let linked = self.links.iter().any(|l| l.r#ref == r);
            if let Some(row) = prs::error_row(
                &pr,
                self.rows.get(r),
                error,
                detail,
                login.as_deref(),
                linked,
                now,
            ) {
                new_rows.push(row);
            }
        }
        for row in new_rows {
            self.save_row(row);
        }
        // My open PRs that are no longer in a viewer list I just read (and no task links).
        let stale: Vec<String> = self
            .rows
            .values()
            .filter(|r| !r.linked && r.error.is_none())
            .filter(|r| {
                r.account
                    .as_deref()
                    .and_then(|a| viewer_ok.get(a))
                    .is_some_and(|mine| !mine.contains(&r.pr))
            })
            .map(|r| r.pr.clone())
            .collect();
        for r in stale {
            if self.db.delete_pr(&r).is_ok() {
                self.rows.remove(&r);
            }
        }
    }

    fn save_row(&mut self, row: PrRow) {
        if self.rows.get(&row.pr).is_some_and(|old| old.same(&row)) {
            return;
        }
        match self.db.save_pr(&row) {
            Ok(()) => {
                self.rows.insert(row.pr.clone(), row);
            }
            Err(e) => log(&format!("couldn't save PR {}: {e}", row.pr)),
        }
    }

    // ---- health ----

    fn gh_off(&mut self, off: GhOff, now: SystemTime) {
        match off {
            GhOff::Missing => self.part.gh.set(
                HealthState::Off,
                "Install the GitHub CLI to sync pull requests.",
                now,
            ),
            GhOff::SignedOut => {
                self.part
                    .gh
                    .set(HealthState::Off, "Run `gh auth login` in a terminal.", now)
            }
            GhOff::Failed(why) => self.part.gh.fail(why, now),
        }
        self.save_health(GH_SCOPE, now);
        self.publish(now);
    }

    fn health(&self, login: &str) -> Option<&Health> {
        self.part
            .accounts
            .iter()
            .find(|(l, _)| l == login)
            .map(|(_, h)| h)
    }

    fn account_mut(&mut self, login: &str, now: SystemTime) -> &mut Health {
        let i = match self.part.accounts.iter().position(|(l, _)| l == login) {
            Some(i) => i,
            None => {
                self.part
                    .accounts
                    .push((login.to_string(), Health::unknown(now)));
                self.part.accounts.len() - 1
            }
        };
        &mut self.part.accounts[i].1
    }

    /// Keeps one health per account `gh` knows, in its order; forgets the rest.
    fn sync_accounts(&mut self, accounts: &[Account], now: SystemTime) {
        let gone: Vec<String> = self
            .part
            .accounts
            .iter()
            .map(|(l, _)| l.clone())
            .filter(|l| !accounts.iter().any(|a| &a.login == l))
            .collect();
        for login in gone {
            let _ = self.db.delete_health("gh", &login);
            self.saved_health.remove(&login);
        }
        let mut next = Vec::new();
        for a in accounts {
            let mut h = self
                .health(&a.login)
                .cloned()
                .unwrap_or_else(|| Health::unknown(now));
            if !a.ok {
                h.set(HealthState::Broken, "Signed out", now);
            }
            next.push((a.login.clone(), h));
        }
        self.part.accounts = next;
        for a in accounts {
            self.save_health_of(&a.login, now);
        }
    }

    fn save_health(&mut self, scope: &str, now: SystemTime) {
        let h = self.part.gh.clone();
        self.write_health(scope, &h, now);
    }

    fn save_health_of(&mut self, login: &str, now: SystemTime) {
        if let Some(h) = self.health(login).cloned() {
            self.write_health(login, &h, now);
        }
    }

    fn write_health(&mut self, scope: &str, h: &Health, _now: SystemTime) {
        let key = (h.state, h.detail.clone());
        if self.saved_health.get(scope) == Some(&key) {
            return;
        }
        match self.db.save_health("gh", scope, h) {
            Ok(()) => {
                self.saved_health.insert(scope.to_string(), key);
            }
            Err(e) => log(&format!("couldn't save gh health: {e}")),
        }
    }

    // ---- publishing ----

    fn publish(&mut self, now: SystemTime) {
        // Rows whose `linked` flag no longer matches the links.
        let linked: HashSet<&str> = self.links.iter().map(|l| l.r#ref.as_str()).collect();
        let flips: Vec<PrRow> = self
            .rows
            .values()
            .filter(|r| r.linked != linked.contains(r.pr.as_str()))
            .map(|r| PrRow {
                linked: !r.linked,
                ..r.clone()
            })
            .collect();
        for row in flips {
            if !row.linked && row.state != Some(PrState::Open) {
                // Unlinked and not one of my open PRs: nothing to show.
                if self.db.delete_pr(&row.pr).is_ok() {
                    self.rows.remove(&row.pr);
                }
            } else {
                self.save_row(row);
            }
        }
        let recent = |task: &TaskId| {
            self.done
                .get(task)
                .and_then(|t| from_ts(t))
                .is_some_and(|t| now.duration_since(t).map_or(true, |d| d <= RECENT_DONE))
        };
        let mut pr_tasks: HashMap<PrRef, Vec<TaskId>> = HashMap::new();
        let mut unread: Vec<&Link> = Vec::new();
        for l in &self.links {
            if self.open.contains(&l.task) || recent(&l.task) {
                if let Some(pr) = PrRef::parse(&l.r#ref) {
                    pr_tasks.entry(pr).or_default().push(l.task);
                }
                if !self.rows.contains_key(&l.r#ref) {
                    unread.push(l);
                }
            }
        }
        for tasks in pr_tasks.values_mut() {
            tasks.sort();
            tasks.dedup();
        }
        let account_ok =
            |a: Option<&str>| a.is_none_or(|a| self.health(a).is_some_and(Health::is_ok));
        let mut views: HashMap<PrRef, airboss_services::PrView> = self
            .rows
            .values()
            .filter_map(|r| prs::view(r, !account_ok(r.account.as_deref())))
            .map(|v| (v.r#ref.clone(), v))
            .collect();
        for l in unread {
            if let Some(v) = prs::unread(l) {
                views.entry(v.r#ref.clone()).or_insert(v);
            }
        }
        self.part.prs = views;
        self.part.pr_tasks = pr_tasks;
        self.env.hub.publish_prs(&self.part);
    }
}
