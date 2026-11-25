//! `sync`'s own tables, on its own connection to the app's database file.
//!
//! The migration runs through `Store::migrate_module("sync", …)`, so `core` records it; the
//! tables are then read and written on a connection of our own, opened at `Store::path()`.
//! `core`'s tables are never touched here.

use std::path::Path;
use std::sync::PoisonError;
use std::time::{Duration, SystemTime};

use airboss_core::{Migration, PrState, TaskId};
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{
    Checks, ErrorCode, Request, RequestKind, Review, ServiceError, SharedStore, Source, ThreadKey,
    ThreadState, ThreadView,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::health::{Health, HealthState};

/// `sync`'s migrations, oldest first. Version 1 hasn't shipped, so it may still be edited.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("migrations/001_sync.sql"),
}];

/// The module name the migrations are recorded under.
pub const MODULE: &str = "sync";

/// One connection to `sync`'s tables.
pub struct SyncDb {
    pub(crate) conn: Connection,
}

impl SyncDb {
    /// Runs `sync`'s migrations on the shared store's file, then opens a connection of our
    /// own on it. An in-memory store is refused: there's no file to open.
    pub fn open(store: &SharedStore) -> Result<SyncDb, ServiceError> {
        let path = {
            let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
            store
                .migrate_module(MODULE, MIGRATIONS)
                .map_err(|e| storage(format!("sync's tables couldn't be set up: {e}")))?;
            store.path().ok_or_else(|| {
                storage("sync needs the database file; this store is in memory".to_string())
            })?
        };
        SyncDb::open_path(&path)
    }

    /// Our own connection on a file whose migrations already ran.
    pub(crate) fn open_path(path: &Path) -> Result<SyncDb, ServiceError> {
        let conn = Connection::open(path)
            .map_err(|e| storage(format!("sync couldn't open {}: {e}", path.display())))?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|e| storage(e.to_string()))?;
        Ok(SyncDb { conn })
    }
}

/// What a failed statement on our own tables becomes. Our tables are ours alone, so this only
/// happens when the disk or the file fails.
pub(crate) type DbResult<T> = rusqlite::Result<T>;

impl SyncDb {
    /// Every `sync_health` row: `(source, scope, health)`. Rows with text we can't read are
    /// skipped; the next pass rewrites them.
    pub(crate) fn load_health(&self) -> DbResult<Vec<(String, String, Health)>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, scope, state, detail, since, last_ok_at, failures, next_try_at
             FROM sync_health ORDER BY source, scope",
        )?;
        let rows = stmt.query_map([], |r| {
            let state: String = r.get(2)?;
            let since: String = r.get(4)?;
            let last_ok: Option<String> = r.get(5)?;
            let next: Option<String> = r.get(7)?;
            let failures: i64 = r.get(6)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                HealthState::parse(&state).and_then(|state| {
                    Some(Health {
                        state,
                        detail: r.get(3).ok()?,
                        since: from_ts(&since)?,
                        last_ok_at: last_ok.as_deref().and_then(from_ts),
                        failures: u32::try_from(failures).unwrap_or(0),
                        next_try_at: next.as_deref().and_then(from_ts),
                        stale: true,
                    })
                }),
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (source, scope, health) = row?;
            if let Some(health) = health {
                out.push((source, scope, health));
            }
        }
        Ok(out)
    }

    /// Writes one source's health.
    pub(crate) fn save_health(&self, source: &str, scope: &str, h: &Health) -> DbResult<()> {
        let ts = |t: Option<SystemTime>| t.map(to_ts);
        self.conn.execute(
            "INSERT INTO sync_health (source, scope, state, detail, since, last_ok_at, failures, next_try_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (source, scope) DO UPDATE SET state = ?3, detail = ?4, since = ?5,
               last_ok_at = ?6, failures = ?7, next_try_at = ?8",
            params![
                source,
                scope,
                h.state.as_str(),
                h.detail,
                to_ts(h.since),
                ts(h.last_ok_at),
                h.failures,
                ts(h.next_try_at)
            ],
        )?;
        Ok(())
    }

    /// Forgets one source's health, e.g. a `gh` account that logged out for good.
    pub(crate) fn delete_health(&self, source: &str, scope: &str) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM sync_health WHERE source = ?1 AND scope = ?2",
            params![source, scope],
        )?;
        Ok(())
    }
}

/// A PR a T3 thread lists, waiting for GitHub to read it before it's linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WaitingPr {
    /// Canonical `owner/repo#n`.
    #[serde(rename = "ref")]
    pub pr: String,
    pub url: Option<String>,
    /// T3's `linkedAt`.
    pub linked_at: String,
}

/// One `observed_threads` row: the view, plus what only `sync` keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredThread {
    pub view: ThreadView,
    pub noted_run_id: Option<String>,
}

pub(crate) fn source_text(source: Source) -> &'static str {
    match source {
        Source::T3 => "t3",
        Source::Claude => "claude",
    }
}

pub(crate) fn state_text(state: ThreadState) -> &'static str {
    match state {
        ThreadState::Running => "running",
        ThreadState::NeedsMe => "needs_me",
        ThreadState::Errored => "errored",
        ThreadState::Idle => "idle",
        ThreadState::Settled => "settled",
        ThreadState::Gone => "gone",
    }
}

fn parse_state(text: &str) -> Option<ThreadState> {
    Some(match text {
        "running" => ThreadState::Running,
        "needs_me" => ThreadState::NeedsMe,
        "errored" => ThreadState::Errored,
        "idle" => ThreadState::Idle,
        "settled" => ThreadState::Settled,
        "gone" => ThreadState::Gone,
        _ => return None,
    })
}

const THREAD_COLUMNS: &str =
    "source, ref, linked, state, title, project_path, branch, worktree_path,
    parent_ref, child_count, run_id, error_text, request_id, request_kind, request_text,
    request_at, last_activity_at, noted_run_id";

fn thread_row(r: &Row<'_>) -> rusqlite::Result<Option<StoredThread>> {
    let source = match r.get::<_, String>(0)?.as_str() {
        "t3" => Source::T3,
        "claude" => Source::Claude,
        _ => return Ok(None),
    };
    let Some(state) = parse_state(&r.get::<_, String>(3)?) else {
        return Ok(None);
    };
    let kind: Option<String> = r.get(13)?;
    let request = match (r.get::<_, Option<String>>(12)?, kind.as_deref()) {
        (Some(id), Some(kind)) => Some(Request {
            id,
            kind: if kind == "question" {
                RequestKind::Question
            } else {
                RequestKind::Approval
            },
            text: r.get::<_, Option<String>>(14)?.unwrap_or_default(),
            at: r.get::<_, Option<String>>(15)?.unwrap_or_default(),
        }),
        _ => None,
    };
    let children: i64 = r.get(9)?;
    Ok(Some(StoredThread {
        view: ThreadView {
            key: ThreadKey {
                source,
                r#ref: r.get(1)?,
            },
            linked: r.get::<_, i64>(2)? == 1,
            state,
            title: r.get(4)?,
            request,
            error: r.get(11)?,
            run_id: r.get(10)?,
            project_path: r.get(5)?,
            branch: r.get(6)?,
            worktree_path: r.get(7)?,
            parent_ref: r.get(8)?,
            child_count: u32::try_from(children).unwrap_or(0),
            last_activity_at: r.get(16)?,
            stale: true,
        },
        noted_run_id: r.get(17)?,
    }))
}

impl SyncDb {
    /// Every `observed_threads` row, marked stale.
    pub(crate) fn load_threads(&self) -> DbResult<Vec<StoredThread>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM observed_threads ORDER BY source, ref"
        ))?;
        let rows = stmt.query_map([], thread_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.extend(row?);
        }
        Ok(out)
    }

    /// Writes a thread's view. `pr_seen`, `pr_waiting` and `noted_run_id` are left alone:
    /// each has its own call, so the two workers never overwrite each other.
    pub(crate) fn save_thread(&self, v: &ThreadView, now: SystemTime) -> DbResult<()> {
        let (rid, rkind, rtext, rat) = match &v.request {
            Some(r) => (
                Some(r.id.as_str()),
                Some(match r.kind {
                    RequestKind::Question => "question",
                    RequestKind::Approval => "approval",
                }),
                Some(r.text.as_str()),
                Some(r.at.as_str()),
            ),
            None => (None, None, None, None),
        };
        self.conn.execute(
            "INSERT INTO observed_threads (source, ref, linked, state, title, project_path, branch,
               worktree_path, parent_ref, child_count, run_id, error_text, request_id,
               request_kind, request_text, request_at, last_activity_at, first_seen_at, changed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?18)
             ON CONFLICT (source, ref) DO UPDATE SET linked = ?3, state = ?4, title = ?5,
               project_path = ?6, branch = ?7, worktree_path = ?8, parent_ref = ?9,
               child_count = ?10, run_id = ?11, error_text = ?12, request_id = ?13,
               request_kind = ?14, request_text = ?15, request_at = ?16, last_activity_at = ?17,
               changed_at = ?18",
            params![
                source_text(v.key.source),
                v.key.r#ref,
                i64::from(v.linked),
                state_text(v.state),
                v.title,
                v.project_path,
                v.branch,
                v.worktree_path,
                v.parent_ref,
                v.child_count,
                v.run_id,
                v.error,
                rid,
                rkind,
                rtext,
                rat,
                v.last_activity_at,
                to_ts(now)
            ],
        )?;
        Ok(())
    }

    pub(crate) fn delete_thread(&self, key: &ThreadKey) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM observed_threads WHERE source = ?1 AND ref = ?2",
            params![source_text(key.source), key.r#ref],
        )?;
        Ok(())
    }

    /// Records that a note was written for this failed run.
    pub(crate) fn set_noted_run(&self, key: &ThreadKey, run_id: &str) -> DbResult<()> {
        self.conn.execute(
            "UPDATE observed_threads SET noted_run_id = ?3 WHERE source = ?1 AND ref = ?2",
            params![source_text(key.source), key.r#ref, run_id],
        )?;
        Ok(())
    }

    /// Adds a T3-listed PR to a thread's `pr_waiting`, unless it's already waiting or seen.
    /// One statement, so it can't race the GitHub worker moving it to `pr_seen`. Returns
    /// whether it was added.
    pub(crate) fn add_waiting(&self, thread: &str, pr: &WaitingPr) -> DbResult<bool> {
        let json = serde_json::to_string(pr)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let n = self.conn.execute(
            "UPDATE observed_threads SET pr_waiting = json_insert(pr_waiting, '$[#]', json(?3))
             WHERE source = 't3' AND ref = ?1
               AND NOT EXISTS (SELECT 1 FROM json_each(pr_seen) WHERE value = ?2)
               AND NOT EXISTS (SELECT 1 FROM json_each(pr_waiting)
                               WHERE json_extract(value, '$.ref') = ?2)",
            params![thread, pr.pr, json],
        )?;
        Ok(n > 0)
    }

    /// Every PR waiting on a T3 thread: `(thread, pr)`.
    pub(crate) fn waiting(&self) -> DbResult<Vec<(String, WaitingPr)>> {
        let mut stmt = self.conn.prepare(
            "SELECT o.ref, w.value FROM observed_threads AS o, json_each(o.pr_waiting) AS w
             WHERE o.source = 't3' ORDER BY o.ref, w.key",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (thread, json) = row?;
            if let Ok(pr) = serde_json::from_str::<WaitingPr>(&json) {
                out.push((thread, pr));
            }
        }
        Ok(out)
    }

    /// Moves a PR from a thread's `pr_waiting` to its `pr_seen`, in one statement.
    pub(crate) fn waiting_to_seen(&self, thread: &str, pr: &str) -> DbResult<()> {
        self.conn.execute(
            "UPDATE observed_threads SET
               pr_waiting = (SELECT json_group_array(json(value)) FROM json_each(pr_waiting)
                             WHERE json_extract(value, '$.ref') IS NOT ?2),
               pr_seen = CASE WHEN EXISTS (SELECT 1 FROM json_each(pr_seen) WHERE value = ?2)
                         THEN pr_seen ELSE json_insert(pr_seen, '$[#]', ?2) END
             WHERE source = 't3' AND ref = ?1",
            params![thread, pr],
        )?;
        Ok(())
    }

    /// A thread's `pr_seen`.
    pub(crate) fn seen(&self, thread: &str) -> DbResult<Vec<String>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT pr_seen FROM observed_threads WHERE source = 't3' AND ref = ?1",
                params![thread],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default())
    }
}

/// Why GitHub couldn't read a PR (`observed_prs.error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrError {
    NotFound,
    NoAccess,
    Failed,
}

/// One `observed_prs` row: what GitHub last said about a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrRow {
    pub pr: String,
    pub linked: bool,
    pub url: Option<String>,
    pub title: Option<String>,
    pub state: Option<PrState>,
    pub draft: bool,
    pub review: Option<Review>,
    pub checks: Option<Checks>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub head_ref: Option<String>,
    pub author: Option<String>,
    pub gh_updated_at: Option<String>,
    pub merged_at: Option<String>,
    pub ready_since: Option<String>,
    pub account: Option<String>,
    pub error: Option<PrError>,
    pub error_detail: Option<String>,
    pub checked_at: String,
}

impl PrRow {
    /// Equal apart from when it was checked.
    pub(crate) fn same(&self, other: &PrRow) -> bool {
        PrRow {
            checked_at: String::new(),
            ..self.clone()
        } == PrRow {
            checked_at: String::new(),
            ..other.clone()
        }
    }
}

fn pr_state_text(s: PrState) -> &'static str {
    s.as_str()
}

fn parse_pr_state(t: &str) -> Option<PrState> {
    Some(match t {
        "open" => PrState::Open,
        "merged" => PrState::Merged,
        "closed" => PrState::Closed,
        _ => return None,
    })
}

fn review_text(r: Review) -> &'static str {
    match r {
        Review::Approved => "approved",
        Review::ChangesRequested => "changes_requested",
        Review::Required => "review_required",
        Review::None => "none",
    }
}

fn parse_review(t: &str) -> Option<Review> {
    Some(match t {
        "approved" => Review::Approved,
        "changes_requested" => Review::ChangesRequested,
        "review_required" => Review::Required,
        "none" => Review::None,
        _ => return None,
    })
}

fn checks_text(c: Checks) -> &'static str {
    match c {
        Checks::Passing => "passing",
        Checks::Failing => "failing",
        Checks::Pending => "pending",
        Checks::None => "none",
    }
}

fn parse_checks(t: &str) -> Option<Checks> {
    Some(match t {
        "passing" => Checks::Passing,
        "failing" => Checks::Failing,
        "pending" => Checks::Pending,
        "none" => Checks::None,
        _ => return None,
    })
}

fn error_text(e: PrError) -> &'static str {
    match e {
        PrError::NotFound => "not_found",
        PrError::NoAccess => "no_access",
        PrError::Failed => "failed",
    }
}

fn parse_error(t: &str) -> Option<PrError> {
    Some(match t {
        "not_found" => PrError::NotFound,
        "no_access" => PrError::NoAccess,
        "failed" => PrError::Failed,
        _ => return None,
    })
}

const PR_COLUMNS: &str = "ref, linked, url, title, state, draft, review, checks, additions,
    deletions, head_ref, author, gh_updated_at, merged_at, ready_since, account, error,
    error_detail, checked_at";

fn pr_row(r: &Row<'_>) -> rusqlite::Result<PrRow> {
    let text = |i: usize| -> rusqlite::Result<Option<String>> { r.get(i) };
    let count = |i: usize| -> rusqlite::Result<Option<u32>> {
        Ok(r.get::<_, Option<i64>>(i)?
            .and_then(|n| u32::try_from(n).ok()))
    };
    Ok(PrRow {
        pr: r.get(0)?,
        linked: r.get::<_, i64>(1)? == 1,
        url: text(2)?,
        title: text(3)?,
        state: text(4)?.as_deref().and_then(parse_pr_state),
        draft: r.get::<_, i64>(5)? == 1,
        review: text(6)?.as_deref().and_then(parse_review),
        checks: text(7)?.as_deref().and_then(parse_checks),
        additions: count(8)?,
        deletions: count(9)?,
        head_ref: text(10)?,
        author: text(11)?,
        gh_updated_at: text(12)?,
        merged_at: text(13)?,
        ready_since: text(14)?,
        account: text(15)?,
        error: text(16)?.as_deref().and_then(parse_error),
        error_detail: text(17)?,
        checked_at: r.get(18)?,
    })
}

impl SyncDb {
    pub(crate) fn load_prs(&self) -> DbResult<Vec<PrRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PR_COLUMNS} FROM observed_prs ORDER BY ref"
        ))?;
        let rows = stmt.query_map([], pr_row)?;
        rows.collect()
    }

    pub(crate) fn save_pr(&self, p: &PrRow) -> DbResult<()> {
        self.conn.execute(
            &format!(
                "INSERT OR REPLACE INTO observed_prs ({PR_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)"
            ),
            params![
                p.pr,
                i64::from(p.linked),
                p.url,
                p.title,
                p.state.map(pr_state_text),
                i64::from(p.draft),
                p.review.map(review_text),
                p.checks.map(checks_text),
                p.additions,
                p.deletions,
                p.head_ref,
                p.author,
                p.gh_updated_at,
                p.merged_at,
                p.ready_since,
                p.account,
                p.error.map(error_text),
                p.error_detail,
                p.checked_at
            ],
        )?;
        Ok(())
    }

    pub(crate) fn delete_pr(&self, pr: &str) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM observed_prs WHERE ref = ?1", params![pr])?;
        Ok(())
    }

    /// What GitHub last said about a PR's state; `None` if it hasn't been read.
    pub(crate) fn pr_state(&self, pr: &str) -> DbResult<Option<PrState>> {
        let state: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT state FROM observed_prs WHERE ref = ?1",
                params![pr],
                |r| r.get(0),
            )
            .optional()?;
        Ok(state.flatten().as_deref().and_then(parse_pr_state))
    }

    /// A thread's last known state.
    pub(crate) fn thread_state(&self, key: &ThreadKey) -> DbResult<Option<ThreadState>> {
        let state: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM observed_threads WHERE source = ?1 AND ref = ?2",
                params![source_text(key.source), key.r#ref],
                |r| r.get(0),
            )
            .optional()?;
        Ok(state.as_deref().and_then(parse_state))
    }

    /// Tasks whose finish is held.
    pub(crate) fn holds(&self) -> DbResult<Vec<TaskId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT task_id FROM sync_finish_held ORDER BY task_id")?;
        let rows = stmt.query_map([], |r| r.get(0).map(TaskId))?;
        rows.collect()
    }

    pub(crate) fn hold(&self, task: TaskId, now: SystemTime) -> DbResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO sync_finish_held (task_id, since) VALUES (?1, ?2)",
            params![task.0, to_ts(now)],
        )?;
        Ok(())
    }

    pub(crate) fn release(&self, task: TaskId) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM sync_finish_held WHERE task_id = ?1",
            params![task.0],
        )?;
        Ok(())
    }
}

pub(crate) fn storage(message: String) -> ServiceError {
    ServiceError::new(ErrorCode::Storage, message)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn db() -> SyncDb {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0].sql).unwrap();
        SyncDb { conn }
    }

    fn view(r: &str) -> ThreadView {
        ThreadView {
            key: ThreadKey {
                source: Source::T3,
                r#ref: r.into(),
            },
            linked: true,
            state: ThreadState::NeedsMe,
            title: Some("t".into()),
            request: Some(Request {
                id: "q".into(),
                kind: RequestKind::Approval,
                text: "Run a command".into(),
                at: "2026-11-26T10:00:00.000Z".into(),
            }),
            error: None,
            run_id: Some("r1".into()),
            project_path: None,
            branch: Some("b".into()),
            worktree_path: None,
            parent_ref: None,
            child_count: 2,
            last_activity_at: Some("2026-11-26T10:00:00.000Z".into()),
            stale: true,
        }
    }

    #[test]
    fn threads_round_trip_and_keep_their_own_columns() {
        let db = db();
        let now = SystemTime::UNIX_EPOCH;
        let v = view("a");
        db.save_thread(&v, now).unwrap();
        db.set_noted_run(&v.key, "r0").unwrap();
        let mut v2 = v.clone();
        v2.state = ThreadState::Idle;
        v2.request = None;
        db.save_thread(&v2, now).unwrap();
        let rows = db.load_threads().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].view, v2);
        assert_eq!(
            rows[0].noted_run_id.as_deref(),
            Some("r0"),
            "kept by save_thread"
        );
        db.save_thread(&v, now).unwrap();
        assert_eq!(db.load_threads().unwrap()[0].view, v);
        db.delete_thread(&v.key).unwrap();
        assert!(db.load_threads().unwrap().is_empty());
    }

    #[test]
    fn waiting_prs_move_to_seen_once() {
        let db = db();
        db.save_thread(&view("t"), SystemTime::UNIX_EPOCH).unwrap();
        let pr = WaitingPr {
            pr: "a/b#1".into(),
            url: Some("https://github.com/a/b/pull/1".into()),
            linked_at: "2026-11-26T10:00:00.000Z".into(),
        };
        assert!(db.add_waiting("t", &pr).unwrap());
        assert!(!db.add_waiting("t", &pr).unwrap(), "already waiting");
        assert!(!db.add_waiting("missing", &pr).unwrap(), "no such thread");
        assert_eq!(db.waiting().unwrap(), vec![("t".to_string(), pr.clone())]);
        db.waiting_to_seen("t", "a/b#1").unwrap();
        db.waiting_to_seen("t", "a/b#1").unwrap();
        assert!(db.waiting().unwrap().is_empty());
        assert_eq!(db.seen("t").unwrap(), vec!["a/b#1".to_string()]);
        assert!(!db.add_waiting("t", &pr).unwrap(), "seen: never again");
    }

    #[test]
    fn health_round_trips() {
        let db = db();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_000_000);
        let mut h = Health::unknown(t0);
        h.fail("no network", t0);
        db.save_health("gh", "me", &h).unwrap();
        h.ok(t0 + Duration::from_secs(60));
        db.save_health("t3", "", &h).unwrap();
        let rows = db.load_health().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "gh");
        assert_eq!(rows[0].2.state, HealthState::Broken);
        assert_eq!(rows[0].2.detail.as_deref(), Some("no network"));
        assert_eq!(rows[1].2.last_ok_at, Some(t0 + Duration::from_secs(60)));
        assert!(rows[1].2.stale, "loaded rows are stale until the next pass");
        db.delete_health("gh", "me").unwrap();
        assert_eq!(db.load_health().unwrap().len(), 1);
    }
}
