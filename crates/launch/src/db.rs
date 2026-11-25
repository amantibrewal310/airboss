//! Launch's own connection and tables: `launch_threads` and `launch_repos`.
//!
//! The schema is migration `launch` v1, run through `core`'s `migrate_module` on the same
//! file as `core`'s tables. After that, launch reads and writes its own tables only, on its
//! own connection.

use std::path::Path;
use std::time::{Duration, SystemTime};

use airboss_core::{Migration, RepoId, Store, TaskId};
use airboss_services::{Mode, ServiceError, time};
use rusqlite::{Connection, OptionalExtension, Row as SqlRow, TransactionBehavior, params};

use crate::errors;

/// Launch's migrations, run with `Store::migrate_module("launch", MIGRATIONS)`.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("migrations/001_launch.sql"),
}];

/// Where one row stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// `t3` may have made a thread; nothing else starts one for this place.
    Pending,
    Started,
    /// `t3` said no thread exists.
    Failed,
    /// No thread was found in time, or "Start again" replaced it.
    Lost,
}

impl State {
    fn parse(text: &str) -> Option<State> {
        Some(match text {
            "pending" => State::Pending,
            "started" => State::Started,
            "failed" => State::Failed,
            "lost" => State::Lost,
            _ => return None,
        })
    }
}

pub(crate) fn mode_str(mode: Mode) -> &'static str {
    match mode {
        Mode::DoIt => "do",
        Mode::PlanIt => "plan",
    }
}

fn parse_mode(text: &str) -> Option<Mode> {
    match text {
        "do" => Some(Mode::DoIt),
        "plan" => Some(Mode::PlanIt),
        _ => None,
    }
}

/// One `launch_threads` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub id: i64,
    pub task: TaskId,
    pub repo: Option<RepoId>,
    pub mode: Mode,
    pub state: State,
    pub thread_id: Option<String>,
    pub project_path: String,
    pub branch: Option<String>,
    pub title: String,
    pub linked: bool,
    pub created_at: SystemTime,
}

/// What a new pending row records.
#[derive(Debug, Clone)]
pub(crate) struct NewRow<'a> {
    pub task: TaskId,
    pub repo: Option<RepoId>,
    pub mode: Mode,
    pub project_path: &'a str,
    /// The branch and its base, for Do it in a repo.
    pub branch: Option<(&'a str, &'a str)>,
    pub title: &'a str,
}

/// What `insert_pending` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Inserted {
    /// The new pending row's id.
    Row(i64),
    /// A pending row already holds this place.
    Pending,
    /// A thread already started here (and isn't being replaced).
    Started,
}

const COLUMNS: &str = "id, task_id, repo_id, mode, state, thread_id, project_path, branch, \
                       title, linked, created_at";

fn from_row(r: &SqlRow<'_>) -> rusqlite::Result<Row> {
    let bad = |i: usize, text: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            i,
            rusqlite::types::Type::Text,
            format!("unknown value {text:?}").into(),
        )
    };
    let mode: String = r.get(3)?;
    let state: String = r.get(4)?;
    let created: String = r.get(10)?;
    Ok(Row {
        id: r.get(0)?,
        task: TaskId(r.get(1)?),
        repo: r.get::<_, Option<i64>>(2)?.map(RepoId),
        mode: parse_mode(&mode).ok_or_else(|| bad(3, &mode))?,
        state: State::parse(&state).ok_or_else(|| bad(4, &state))?,
        thread_id: r.get(5)?,
        project_path: r.get(6)?,
        branch: r.get(7)?,
        title: r.get(8)?,
        linked: r.get::<_, i64>(9)? == 1,
        created_at: time::from_ts(&created).ok_or_else(|| bad(10, &created))?,
    })
}

/// Launch's connection to its tables.
pub(crate) struct Db {
    conn: Connection,
}

type Result<T> = std::result::Result<T, ServiceError>;

fn storage(e: rusqlite::Error) -> ServiceError {
    errors::storage(&e)
}

impl Db {
    /// Runs launch's migrations on `path` (through `core`), then opens launch's connection.
    pub fn open(path: &Path) -> Result<Db> {
        let mut store = Store::open(path).map_err(|e| errors::core(&e))?;
        store
            .migrate_module("launch", MIGRATIONS)
            .map_err(|e| errors::core(&e))?;
        drop(store);
        let conn = Connection::open(path).map_err(storage)?;
        conn.busy_timeout(Duration::from_millis(2000))
            .map_err(storage)?;
        Ok(Db { conn })
    }

    fn query(&self, sql: &str, args: impl rusqlite::Params) -> Result<Vec<Row>> {
        let mut stmt = self.conn.prepare_cached(sql).map_err(storage)?;
        let rows = stmt
            .query_map(args, from_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<Row>>>()
            .map_err(storage)?;
        Ok(rows)
    }

    /// Every row of one task, oldest first.
    pub fn rows_for_task(&self, task: TaskId) -> Result<Vec<Row>> {
        self.query(
            &format!("SELECT {COLUMNS} FROM launch_threads WHERE task_id = ?1 ORDER BY id"),
            params![task.0],
        )
    }

    /// Every pending row, oldest first.
    pub fn pending(&self) -> Result<Vec<Row>> {
        self.query(
            &format!("SELECT {COLUMNS} FROM launch_threads WHERE state = 'pending' ORDER BY id"),
            [],
        )
    }

    /// Every started row core doesn't have the link for yet.
    pub fn unlinked(&self) -> Result<Vec<Row>> {
        self.query(
            &format!(
                "SELECT {COLUMNS} FROM launch_threads WHERE state = 'started' AND linked = 0 \
                 ORDER BY id"
            ),
            [],
        )
    }

    /// Commits a pending row, unless the place is taken, in one IMMEDIATE transaction, so
    /// two calls at once can't both pass the check.
    ///
    /// Without `again`, a started or pending row takes the place. With `again`, the rows the
    /// caller saw (ids up to `seen`) are replaced: their pending rows become `lost`. A row
    /// made after that, by a call running at the same time, still takes the place.
    pub fn insert_pending(
        &mut self,
        new: &NewRow<'_>,
        again: bool,
        seen: i64,
        now: SystemTime,
    ) -> Result<Inserted> {
        let at = time::to_ts(now);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let after = if again { seen } else { 0 };
        let taken: Option<String> = tx
            .query_row(
                "SELECT state FROM launch_threads
                 WHERE task_id = ?1 AND ifnull(repo_id, 0) = ifnull(?2, 0) AND mode = ?3
                   AND state IN ('started', 'pending') AND id > ?4
                 ORDER BY state = 'started' DESC LIMIT 1",
                params![new.task.0, new.repo.map(|r| r.0), mode_str(new.mode), after],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        match taken.as_deref() {
            Some("started") => return Ok(Inserted::Started),
            Some(_) => return Ok(Inserted::Pending),
            None => {}
        }
        if again {
            tx.execute(
                "UPDATE launch_threads SET state = 'lost', settled_at = ?4
                 WHERE task_id = ?1 AND ifnull(repo_id, 0) = ifnull(?2, 0) AND mode = ?3
                   AND state = 'pending'",
                params![new.task.0, new.repo.map(|r| r.0), mode_str(new.mode), at],
            )
            .map_err(storage)?;
        }
        let inserted = tx.execute(
            "INSERT INTO launch_threads
               (task_id, repo_id, mode, state, project_path, branch, base_ref, title, created_at)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8)",
            params![
                new.task.0,
                new.repo.map(|r| r.0),
                mode_str(new.mode),
                new.project_path,
                new.branch.map(|(b, _)| b),
                new.branch.map(|(_, base)| base),
                new.title,
                at
            ],
        );
        match inserted {
            Ok(_) => {
                let id = tx.last_insert_rowid();
                tx.commit().map_err(storage)?;
                Ok(Inserted::Row(id))
            }
            // Only the one-pending index can refuse it; the check above makes this rare.
            Err(rusqlite::Error::SqliteFailure(f, _))
                if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Ok(Inserted::Pending)
            }
            Err(e) => Err(storage(e)),
        }
    }

    /// The row became `started` with `thread`. A `pending` or `lost` row takes the thread id
    /// (a thread that exists wins); a row already `started` keeps the id it has. Returns the
    /// row as it now is, or `None` if the thread id already belongs to another row.
    pub fn started(&mut self, id: i64, thread: &str, now: SystemTime) -> Result<Option<Row>> {
        let changed = self.conn.execute(
            "UPDATE launch_threads SET state = 'started', thread_id = ?2, error = NULL,
                    settled_at = ?3
             WHERE id = ?1 AND state IN ('pending', 'lost')",
            params![id, thread, time::to_ts(now)],
        );
        match changed {
            Ok(_) => self.row(id),
            Err(rusqlite::Error::SqliteFailure(f, _))
                if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Ok(None)
            }
            Err(e) => Err(storage(e)),
        }
    }

    /// A pending row became `failed`: `t3` said no thread exists.
    pub fn failed(&mut self, id: i64, error: &str, now: SystemTime) -> Result<()> {
        self.conn
            .execute(
                "UPDATE launch_threads SET state = 'failed', error = ?2, settled_at = ?3
                 WHERE id = ?1 AND state = 'pending'",
                params![id, error, time::to_ts(now)],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// A pending row became `lost`. `false` if it wasn't pending any more.
    pub fn lost(&mut self, id: i64, now: SystemTime) -> Result<bool> {
        let n = self
            .conn
            .execute(
                "UPDATE launch_threads SET state = 'lost', settled_at = ?2
                 WHERE id = ?1 AND state = 'pending'",
                params![id, time::to_ts(now)],
            )
            .map_err(storage)?;
        Ok(n > 0)
    }

    /// Core has the link now.
    pub fn linked(&mut self, id: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE launch_threads SET linked = 1 WHERE id = ?1 AND state = 'started'",
                params![id],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The task is gone from `core`: forget its rows.
    pub fn forget_task(&mut self, task: TaskId) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM launch_threads WHERE task_id = ?1",
                params![task.0],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn row(&self, id: i64) -> Result<Option<Row>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM launch_threads WHERE id = ?1"),
                params![id],
                from_row,
            )
            .optional()
            .map_err(storage)
    }

    /// Whether any row of this task already used `branch`.
    pub fn branch_used(&self, task: TaskId, branch: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM launch_threads WHERE task_id = ?1 AND branch = ?2)",
                params![task.0, branch],
                |r| r.get(0),
            )
            .map_err(storage)
    }

    /// The repo's base override, if set.
    pub fn base_ref(&self, repo: RepoId) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT base_ref FROM launch_repos WHERE repo_id = ?1",
                params![repo.0],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(storage)
    }

    /// Records the T3 project a repo resolved to.
    pub fn set_t3_project(&mut self, repo: RepoId, project: &str, now: SystemTime) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO launch_repos (repo_id, t3_project_id, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (repo_id) DO UPDATE SET t3_project_id = ?2, updated_at = ?3",
                params![repo.0, project, time::to_ts(now)],
            )
            .map_err(storage)?;
        Ok(())
    }
}
