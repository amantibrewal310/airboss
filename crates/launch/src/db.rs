//! Launch's own connection and tables: `launch_threads` and `launch_repos`.
//!
//! The schema is migrations `launch` v1 to v3, run through `core`'s `migrate_module` on the same
//! file as `core`'s tables. After that, launch reads and writes its own tables only, on its
//! own connection.

use std::path::Path;
use std::time::{Duration, SystemTime};

use airboss_core::{LinkKind, Migration, RepoId, Store, TaskId};
use airboss_services::{Launcher, Mode, ServiceError, Source, time};
use rusqlite::{Connection, OptionalExtension, Row as SqlRow, TransactionBehavior, params};

use crate::errors;

/// Launch's migrations, run with `Store::migrate_module("launch", MIGRATIONS)`.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("migrations/001_launch.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/002_repo_path.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/003_herdr.sql"),
    },
];

/// Where a row's thread runs: a T3 Code thread, or a terminal in Herdr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Runs {
    T3,
    Herdr,
}

impl Runs {
    pub fn as_str(self) -> &'static str {
        match self {
            Runs::T3 => "t3",
            Runs::Herdr => "herdr",
        }
    }

    fn parse(text: &str) -> Option<Runs> {
        match text {
            "t3" => Some(Runs::T3),
            "herdr" => Some(Runs::Herdr),
            _ => None,
        }
    }

    /// The link kind and thread source for this place's threads.
    pub fn link_kind(self) -> LinkKind {
        match self {
            Runs::T3 => LinkKind::T3Thread,
            Runs::Herdr => LinkKind::HerdrTerminal,
        }
    }

    pub fn source(self) -> Source {
        match self {
            Runs::T3 => Source::T3,
            Runs::Herdr => Source::Herdr,
        }
    }
}

impl From<Launcher> for Runs {
    fn from(l: Launcher) -> Runs {
        match l {
            Launcher::T3 => Runs::T3,
            Launcher::Herdr { .. } => Runs::Herdr,
        }
    }
}

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
    pub runs: Runs,
    /// The worktree Herdr's Do it made for the thread.
    pub worktree: Option<String>,
}

impl Row {
    /// The folder the thread's terminal starts in: its worktree, else the place's folder.
    pub fn folder(&self) -> &str {
        self.worktree.as_deref().unwrap_or(&self.project_path)
    }
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
    pub runs: Runs,
    /// The worktree Herdr's Do it made (or will make) for the thread.
    pub worktree: Option<&'a str>,
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
                       title, linked, created_at, launcher, worktree_path";

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
    let runs: String = r.get(11)?;
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
        runs: Runs::parse(&runs).ok_or_else(|| bad(11, &runs))?,
        worktree: r.get(12)?,
    })
}

/// How long to pause before trying a busy write once more.
const BUSY_PAUSE: Duration = Duration::from_millis(100);

/// Runs `write` once more if SQLite was still busy after its own 2-second wait. Used for the
/// writes that record what `t3` confirmed, so a thread that exists isn't left unrecorded.
fn retry_busy<T>(mut write: impl FnMut() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    match write() {
        Err(rusqlite::Error::SqliteFailure(f, _))
            if matches!(
                f.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            std::thread::sleep(BUSY_PAUSE);
            write()
        }
        other => other,
    }
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
               (task_id, repo_id, mode, state, project_path, branch, base_ref, title, created_at,
                launcher, worktree_path)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                new.task.0,
                new.repo.map(|r| r.0),
                mode_str(new.mode),
                new.project_path,
                new.branch.map(|(b, _)| b),
                new.branch.map(|(_, base)| base),
                new.title,
                at,
                new.runs.as_str(),
                new.worktree,
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
        let changed = retry_busy(|| {
            self.conn.execute(
                "UPDATE launch_threads SET state = 'started', thread_id = ?2, error = NULL,
                        settled_at = ?3
                 WHERE id = ?1 AND state IN ('pending', 'lost')",
                params![id, thread, time::to_ts(now)],
            )
        });
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
        retry_busy(|| {
            self.conn.execute(
                "UPDATE launch_threads SET state = 'failed', error = ?2, settled_at = ?3
                 WHERE id = ?1 AND state = 'pending'",
                params![id, error, time::to_ts(now)],
            )
        })
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

    /// Whether any row already has `thread` as its thread.
    pub fn has_thread(&self, thread: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM launch_threads WHERE thread_id = ?1)",
                params![thread],
                |r| r.get(0),
            )
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

    /// The repo's base override, if set. A row kept for this id but another path (or none) is
    /// another repo's, or this one's before it moved, so it's ignored.
    pub fn base_ref(&self, repo: RepoId, path: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT base_ref FROM launch_repos WHERE repo_id = ?1 AND repo_path = ?2",
                params![repo.0, path],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(storage)
    }

    /// Records the T3 project the repo at `path` resolved to. A row this id kept for another
    /// path is taken over, and its base override, which was for that other repo, is dropped.
    pub fn set_t3_project(
        &mut self,
        repo: RepoId,
        path: &str,
        project: &str,
        now: SystemTime,
    ) -> Result<()> {
        // Every right-hand side reads the row as it was, so the CASE sees the old path.
        self.conn
            .execute(
                "INSERT INTO launch_repos (repo_id, repo_path, t3_project_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (repo_id) DO UPDATE SET
                   base_ref = CASE WHEN repo_path IS ?2 THEN base_ref END,
                   repo_path = ?2, t3_project_id = ?3, updated_at = ?4",
                params![repo.0, path, project, time::to_ts(now)],
            )
            .map_err(storage)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::UNIX_EPOCH;

    use super::*;

    /// A launch table file in its own temp dir, removed when dropped.
    struct TempDb(PathBuf, Db);

    impl TempDb {
        fn new() -> TempDb {
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "airboss-launch-db-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let db = Db::open(&dir.join("launch.db")).unwrap();
            TempDb(dir, db)
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_795_687_200)
    }

    fn row(repo: i64) -> NewRow<'static> {
        NewRow {
            task: TaskId(1),
            repo: Some(RepoId(repo)),
            mode: Mode::DoIt,
            project_path: "/src/web",
            branch: Some(("task-1-x", "main")),
            title: "1 X",
            runs: Runs::T3,
            worktree: None,
        }
    }

    fn insert(db: &mut Db, again: bool, seen: i64) -> Inserted {
        db.insert_pending(&row(1), again, seen, now()).unwrap()
    }

    fn states(db: &Db) -> Vec<State> {
        db.rows_for_task(TaskId(1))
            .unwrap()
            .into_iter()
            .map(|r| r.state)
            .collect()
    }

    #[test]
    fn without_again_a_started_row_takes_the_place() {
        let mut t = TempDb::new();
        let Inserted::Row(id) = insert(&mut t.1, false, 0) else {
            panic!("the first insert goes in");
        };
        t.1.started(id, "thread-1", now()).unwrap();
        // A call that read the rows before this one finished still can't add a second.
        assert_eq!(insert(&mut t.1, false, 0), Inserted::Started);
        assert_eq!(states(&t.1), [State::Started]);
    }

    #[test]
    fn without_again_a_pending_row_takes_the_place() {
        let mut t = TempDb::new();
        assert!(matches!(insert(&mut t.1, false, 0), Inserted::Row(_)));
        assert_eq!(insert(&mut t.1, false, 0), Inserted::Pending);
        // Another repo is another place.
        let other = t.1.insert_pending(&row(2), false, 0, now()).unwrap();
        assert!(matches!(other, Inserted::Row(_)));
    }

    #[test]
    fn again_doesnt_replace_a_row_newer_than_it_saw() {
        let mut t = TempDb::new();
        let Inserted::Row(old) = insert(&mut t.1, false, 0) else {
            panic!();
        };
        t.1.lost(old, now()).unwrap();
        // Another call starts the place after this one looked (it saw only `old`).
        let Inserted::Row(newer) = insert(&mut t.1, false, 0) else {
            panic!();
        };
        assert_eq!(insert(&mut t.1, true, old), Inserted::Pending);
        t.1.started(newer, "thread-2", now()).unwrap();
        assert_eq!(insert(&mut t.1, true, old), Inserted::Started);
        assert_eq!(states(&t.1), [State::Lost, State::Started]);
    }

    #[test]
    fn again_with_everything_seen_replaces_pending_rows() {
        let mut t = TempDb::new();
        let Inserted::Row(first) = insert(&mut t.1, false, 0) else {
            panic!();
        };
        let Inserted::Row(second) = insert(&mut t.1, true, first) else {
            panic!("again replaces the pending row it saw");
        };
        assert_eq!(states(&t.1), [State::Lost, State::Pending]);
        // A started row it saw doesn't stop it either.
        t.1.started(second, "thread-1", now()).unwrap();
        assert!(matches!(insert(&mut t.1, true, second), Inserted::Row(_)));
        assert_eq!(states(&t.1), [State::Lost, State::Started, State::Pending]);
    }

    #[test]
    fn a_lost_row_takes_a_thread_t3_names_later() {
        let mut t = TempDb::new();
        let Inserted::Row(id) = insert(&mut t.1, false, 0) else {
            panic!();
        };
        assert!(t.1.lost(id, now()).unwrap());
        // t3 answers after the row was given up on (the Mac slept mid-call).
        let row = t.1.started(id, "late-thread", now()).unwrap().unwrap();
        assert_eq!(row.state, State::Started);
        assert_eq!(row.thread_id.as_deref(), Some("late-thread"));
    }

    #[test]
    fn a_started_row_keeps_its_thread() {
        let mut t = TempDb::new();
        let Inserted::Row(id) = insert(&mut t.1, false, 0) else {
            panic!();
        };
        t.1.started(id, "found-by-sync", now()).unwrap();
        let row = t.1.started(id, "from-t3", now()).unwrap().unwrap();
        assert_eq!(row.thread_id.as_deref(), Some("found-by-sync"));
    }

    #[test]
    fn a_failed_row_records_why() {
        let mut t = TempDb::new();
        let Inserted::Row(id) = insert(&mut t.1, false, 0) else {
            panic!();
        };
        t.1.failed(id, "T3 Code isn't open.", now()).unwrap();
        assert_eq!(states(&t.1), [State::Failed]);
        // Failed rows don't hold the place.
        assert!(matches!(insert(&mut t.1, false, 0), Inserted::Row(_)));
    }
}
