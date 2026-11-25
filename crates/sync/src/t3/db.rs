//! [`T3Db`]: T3's `statev2.sqlite`, opened read-only, and every query `sync` runs on it.
//!
//! Each JSON read goes through [`j`] or [`jt`], so a row whose `payload_json` isn't valid
//! JSON reads as null instead of failing the query. Every value is checked against T3's own
//! lists, and every required key's presence with `json_type`, which is NULL only when the key
//! is missing. Anything unexpected is [`T3Error::Broken`], naming what.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use airboss_services::time::{from_ts, to_ts};
use airboss_services::{T3Project, Timestamp};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};
use serde_json::Value;

use super::{
    REQUEST_STATUSES, RunStatus, T3Error, T3Pr, T3Query, T3Read, T3Request, T3RequestItem,
    T3RequestKind, T3Run, T3Source, T3Thread, T3Untracked,
};

const THREADS: &str = "orchestration_v2_projection_threads";
const RUNS: &str = "orchestration_v2_projection_runs";
const REQUESTS: &str = "orchestration_v2_projection_runtime_requests";
const ITEMS: &str = "orchestration_v2_projection_turn_items";
const PROJECTS: &str = "projection_projects";

/// The columns `sync` reads, per table.
const COLUMNS: &[(&str, &[&str])] = &[
    (
        THREADS,
        &[
            "thread_id",
            "project_id",
            "title",
            "created_at",
            "updated_at",
            "archived_at",
            "deleted_at",
            "payload_json",
        ],
    ),
    (
        RUNS,
        &[
            "run_id",
            "thread_id",
            "ordinal",
            "status",
            "requested_at",
            "completed_at",
            "payload_json",
        ],
    ),
    (
        REQUESTS,
        &[
            "runtime_request_id",
            "thread_id",
            "node_id",
            "kind",
            "status",
            "created_at",
        ],
    ),
    (
        ITEMS,
        &[
            "thread_id",
            "run_id",
            "node_id",
            "type",
            "status",
            "updated_at",
            "payload_json",
        ],
    ),
    (
        PROJECTS,
        &[
            "project_id",
            "title",
            "workspace_root",
            "updated_at",
            "deleted_at",
            "archived_at",
        ],
    ),
];

/// The keys every PR entry in a thread's `$.pullRequests` must have.
const PR_KEYS: &[&str] = &["url", "linkedAt", "repository", "number", "host"];

/// `json_extract` of `path` in `alias.payload_json`, or NULL when the payload isn't JSON.
fn j(alias: &str, path: &str) -> String {
    guarded(alias, "json_extract", path)
}

/// `json_type` of `path`: NULL only when the key is missing (a JSON null is `'null'`).
fn jt(alias: &str, path: &str) -> String {
    guarded(alias, "json_type", path)
}

fn guarded(alias: &str, func: &str, path: &str) -> String {
    format!(
        "CASE WHEN json_valid({alias}.payload_json) THEN {func}({alias}.payload_json, '{path}') END"
    )
}

/// The queries, built once.
struct Queries {
    q1: String,
    q2: String,
    q3: String,
    q4: String,
    q5: String,
    q6: String,
    q7: String,
    q8: String,
}

impl Queries {
    fn new() -> Queries {
        Queries {
            q1: format!(
                "SELECT t.thread_id, t.title, t.updated_at,
                        t.archived_at IS NOT NULL OR t.deleted_at IS NOT NULL,
                        {b}, {w}, {p}, {s}, {prs}, pr.workspace_root,
                        {tb}, {tw}, {tp}, {tr}, {ts}
                 FROM json_each(?1) AS w JOIN {THREADS} AS t ON t.thread_id = w.value
                 LEFT JOIN {PROJECTS} AS pr ON pr.project_id = t.project_id",
                b = j("t", "$.branch"),
                w = j("t", "$.worktreePath"),
                p = j("t", "$.lineage.parentThreadId"),
                s = j("t", "$.settledAt"),
                prs = j("t", "$.pullRequests"),
                tb = jt("t", "$.branch"),
                tw = jt("t", "$.worktreePath"),
                tp = jt("t", "$.lineage.parentThreadId"),
                tr = jt("t", "$.lineage.rootThreadId"),
                ts = jt("t", "$.settledAt"),
            ),
            q2: format!(
                "SELECT r.thread_id, r.run_id, r.status, r.requested_at, {s}, {ts}, r.completed_at
                 FROM json_each(?1) AS w JOIN {RUNS} AS r ON r.thread_id = w.value
                  AND r.ordinal = (SELECT max(x.ordinal) FROM {RUNS} AS x WHERE x.thread_id = w.value)",
                s = j("r", "$.startedAt"),
                ts = jt("r", "$.startedAt"),
            ),
            q3: format!(
                "SELECT q.runtime_request_id, q.thread_id, {root}, q.kind, q.created_at, i.type,
                        {question}, {tquestion}, {prompt}
                 FROM {REQUESTS} AS q
                 JOIN {THREADS} AS t ON t.thread_id = q.thread_id
                 LEFT JOIN {ITEMS} AS i
                   ON i.node_id = q.node_id AND i.type IN ('user_input_request', 'approval_request')
                 WHERE q.status = 'pending' ORDER BY q.created_at",
                root = j("t", "$.lineage.rootThreadId"),
                question = j("i", "$.questions[0].question"),
                tquestion = jt("i", "$.questions[0].question"),
                prompt = j("i", "$.prompt"),
            ),
            q4: format!(
                "SELECT {m}, {tm} FROM {ITEMS} AS e
                 WHERE e.thread_id = ?1 AND e.run_id = ?2 AND e.type = 'error' AND e.status = 'failed'
                 ORDER BY e.updated_at DESC LIMIT 1",
                m = j("e", "$.failure.message"),
                tm = jt("e", "$.failure.message"),
            ),
            q5: format!(
                "SELECT {root}, count(*) FROM {THREADS} AS t
                 WHERE {root} IN (SELECT value FROM json_each(?1))
                   AND t.thread_id NOT IN (SELECT value FROM json_each(?1)) AND t.deleted_at IS NULL
                 GROUP BY 1",
                root = j("t", "$.lineage.rootThreadId"),
            ),
            q6: format!(
                "SELECT t.thread_id, t.title, t.created_at, t.updated_at, {s}, {b}, p.workspace_root
                 FROM {THREADS} AS t LEFT JOIN {PROJECTS} AS p ON p.project_id = t.project_id
                 WHERE t.deleted_at IS NULL AND t.archived_at IS NULL AND t.updated_at >= ?1
                   AND {parent} IS NULL AND coalesce({side}, 0) = 0
                 ORDER BY t.updated_at DESC LIMIT 200",
                s = j("t", "$.settledAt"),
                b = j("t", "$.branch"),
                parent = j("t", "$.lineage.parentThreadId"),
                side = j("t", "$.lineage.sideChat"),
            ),
            q7: format!(
                "SELECT t.thread_id FROM {THREADS} AS t
                 WHERE t.project_id = ?1 AND t.deleted_at IS NULL AND {b} = ?2
                 ORDER BY t.created_at DESC LIMIT 1",
                b = j("t", "$.branch"),
            ),
            q8: format!(
                "SELECT project_id, title, workspace_root FROM {PROJECTS}
                 WHERE deleted_at IS NULL AND archived_at IS NULL ORDER BY updated_at DESC"
            ),
        }
    }
}

/// Q0: every run status, request status and request kind in the database.
const Q0: &str = "SELECT 'run_status', status FROM orchestration_v2_projection_runs GROUP BY status
UNION ALL SELECT 'request_status', status FROM orchestration_v2_projection_runtime_requests GROUP BY status
UNION ALL SELECT 'request_kind', kind FROM orchestration_v2_projection_runtime_requests GROUP BY kind";

/// T3's database, read-only. Opened with `mode=ro`, `SQLITE_OPEN_READ_ONLY` and
/// `query_only`; never `immutable`, which would skip the WAL. Each read is one short
/// transaction, so T3's checkpoints are never held back.
pub struct T3Db {
    dir: PathBuf,
    conn: Option<Connection>,
    queries: Queries,
}

impl T3Db {
    /// T3's data folder, `~/.t3/userdata`. Nothing is opened until [`T3Source::open`].
    pub fn new(t3_dir: &Path) -> T3Db {
        T3Db {
            dir: t3_dir.to_path_buf(),
            conn: None,
            queries: Queries::new(),
        }
    }

    /// `statev2.sqlite` in T3's folder.
    pub fn database_path(t3_dir: &Path) -> PathBuf {
        t3_dir.join("statev2.sqlite")
    }

    /// The open connection (opening it first if needed) and the queries.
    fn parts(&mut self) -> Result<(&Connection, &Queries), T3Error> {
        if self.conn.is_none() {
            self.open()?;
        }
        match &self.conn {
            Some(conn) => Ok((conn, &self.queries)),
            None => Err(T3Error::Broken("T3's database isn't open".into())),
        }
    }
}

/// The `file:` URI SQLite opens, with the characters a URI gives meaning to escaped.
fn uri(path: &Path) -> String {
    let mut out = String::from("file:");
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            _ => out.push(c),
        }
    }
    out.push_str("?mode=ro");
    out
}

/// A query error: busy is its own case, anything else means T3's layout isn't what we know.
fn sql_error(what: &str, e: rusqlite::Error) -> T3Error {
    match e.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            T3Error::Busy
        }
        _ => T3Error::Broken(format!("{what} failed: {e}")),
    }
}

fn broken(what: impl Into<String>) -> T3Error {
    T3Error::Broken(what.into())
}

/// T3's text time in our format, or `Broken` naming where it came from.
fn time(text: &str, what: &str) -> Result<Timestamp, T3Error> {
    from_ts(text)
        .map(to_ts)
        .ok_or_else(|| broken(format!("{what} has a time sync can't read: {text:?}")))
}

fn opt_time(text: Option<String>, what: &str) -> Result<Option<Timestamp>, T3Error> {
    text.map(|t| time(&t, what)).transpose()
}

/// A required key must be present (its `json_type` isn't NULL); its value may be null.
fn require(present: Option<String>, key: &str, table: &str, id: &str) -> Result<(), T3Error> {
    match present {
        Some(_) => Ok(()),
        None => Err(broken(format!("T3 {table} row {id:?} has no {key} key"))),
    }
}

/// A run status from T3's list, or `Broken`.
fn run_status(text: &str) -> Result<RunStatus, T3Error> {
    RunStatus::parse(text).ok_or_else(|| broken(format!("unknown T3 run status {text:?}")))
}

fn request_kind(text: &str) -> Result<T3RequestKind, T3Error> {
    T3RequestKind::parse(text).ok_or_else(|| broken(format!("unknown T3 request kind {text:?}")))
}

fn json_list(ids: &[String]) -> String {
    Value::Array(ids.iter().cloned().map(Value::String).collect()).to_string()
}

/// A thread's `$.pullRequests`, each entry with every required key.
fn pull_requests(json: Option<String>, thread: &str) -> Result<Vec<T3Pr>, T3Error> {
    let Some(json) = json else {
        return Ok(Vec::new());
    };
    let value: Value = serde_json::from_str(&json).map_err(|e| {
        broken(format!(
            "thread {thread:?} has unreadable pullRequests: {e}"
        ))
    })?;
    let Value::Array(entries) = value else {
        return Err(broken(format!(
            "thread {thread:?} has pullRequests that isn't a list"
        )));
    };
    let mut out = Vec::new();
    for entry in entries {
        let Value::Object(map) = entry else {
            return Err(broken(format!(
                "thread {thread:?} has a pullRequests entry that isn't an object"
            )));
        };
        if let Some(key) = PR_KEYS.iter().find(|k| !map.contains_key(**k)) {
            return Err(broken(format!(
                "thread {thread:?} has a pullRequests entry with no {key} key"
            )));
        }
        let text = |k: &str| map.get(k).and_then(Value::as_str).map(str::to_string);
        out.push(T3Pr {
            host: text("host"),
            repository: text("repository"),
            number: map.get("number").and_then(Value::as_u64),
            url: text("url"),
            linked_at: opt_time(text("linkedAt"), &format!("thread {thread:?}'s PR entry"))?,
        });
    }
    Ok(out)
}

/// Every row of a query, through a fallible mapper.
fn rows<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    what: &str,
    mut map: impl FnMut(&Row<'_>) -> Result<T, T3Error>,
) -> Result<Vec<T>, T3Error> {
    let mut stmt = conn.prepare_cached(sql).map_err(|e| sql_error(what, e))?;
    let mut rows = stmt.query(params).map_err(|e| sql_error(what, e))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| sql_error(what, e))? {
        out.push(map(row)?);
    }
    Ok(out)
}

/// A column, with a type mismatch reported as `Broken`.
fn col<T: rusqlite::types::FromSql>(row: &Row<'_>, i: usize, what: &str) -> Result<T, T3Error> {
    row.get(i)
        .map_err(|e| broken(format!("{what} column {i} isn't what sync expects: {e}")))
}

/// The layout and value checks: every column `sync` uses, and Q0's values.
fn check(conn: &Connection) -> Result<(), T3Error> {
    for (table, wanted) in COLUMNS {
        let have: HashSet<String> = rows(
            conn,
            "SELECT name FROM pragma_table_info(?1)",
            params![table],
            "the column check",
            |r| col(r, 0, "pragma_table_info"),
        )?
        .into_iter()
        .collect();
        if have.is_empty() {
            return Err(broken(format!("T3 has no {table} table")));
        }
        if let Some(missing) = wanted.iter().find(|c| !have.contains(**c)) {
            return Err(broken(format!("T3's {table} has no {missing} column")));
        }
    }
    let values = rows(conn, Q0, [], "the value check", |r| {
        Ok((
            col::<String>(r, 0, "Q0")?,
            col::<Option<String>>(r, 1, "Q0")?,
        ))
    })?;
    for (what, value) in values {
        let value = value.unwrap_or_default();
        let known = match what.as_str() {
            "run_status" => RunStatus::parse(&value).is_some(),
            "request_kind" => T3RequestKind::parse(&value).is_some(),
            _ => REQUEST_STATUSES.contains(&value.as_str()),
        };
        if !known {
            let name = what.replace('_', " ");
            return Err(broken(format!("unknown T3 {name} {value:?}")));
        }
    }
    Ok(())
}

impl Queries {
    fn q1(&self, conn: &Connection, linked: &str) -> Result<Vec<T3Thread>, T3Error> {
        rows(conn, &self.q1, params![linked], "Q1 (threads)", |r| {
            let id: String = col(r, 0, "Q1")?;
            for (i, key) in [
                (10, "$.branch"),
                (11, "$.worktreePath"),
                (12, "$.lineage.parentThreadId"),
                (13, "$.lineage.rootThreadId"),
                (14, "$.settledAt"),
            ] {
                require(col(r, i, "Q1")?, key, "thread", &id)?;
            }
            let what = format!("thread {id:?}");
            Ok(T3Thread {
                title: col(r, 1, "Q1")?,
                updated_at: time(&col::<String>(r, 2, "Q1")?, &what)?,
                gone: col(r, 3, "Q1")?,
                branch: col(r, 4, "Q1")?,
                worktree_path: col(r, 5, "Q1")?,
                parent: col(r, 6, "Q1")?,
                settled_at: opt_time(col(r, 7, "Q1")?, &what)?,
                pull_requests: pull_requests(col(r, 8, "Q1")?, &id)?,
                project_path: col(r, 9, "Q1")?,
                id,
            })
        })
    }

    fn q2(&self, conn: &Connection, linked: &str) -> Result<Vec<T3Run>, T3Error> {
        rows(conn, &self.q2, params![linked], "Q2 (runs)", |r| {
            let run_id: String = col(r, 1, "Q2")?;
            require(col(r, 5, "Q2")?, "$.startedAt", "run", &run_id)?;
            let what = format!("run {run_id:?}");
            Ok(T3Run {
                thread_id: col(r, 0, "Q2")?,
                status: run_status(&col::<String>(r, 2, "Q2")?)?,
                requested_at: time(&col::<String>(r, 3, "Q2")?, &what)?,
                started_at: opt_time(col(r, 4, "Q2")?, &what)?,
                completed_at: opt_time(col(r, 6, "Q2")?, &what)?,
                run_id,
            })
        })
    }

    fn q3(&self, conn: &Connection) -> Result<Vec<T3Request>, T3Error> {
        let mut seen = HashSet::new();
        let all = rows(conn, &self.q3, [], "Q3 (requests)", |r| {
            let id: String = col(r, 0, "Q3")?;
            let kind = request_kind(&col::<String>(r, 3, "Q3")?)?;
            let item_type: Option<String> = col(r, 5, "Q3")?;
            let item = match item_type.as_deref() {
                None => None,
                Some("user_input_request") => {
                    require(
                        col(r, 7, "Q3")?,
                        "$.questions[0].question",
                        "question item",
                        &id,
                    )?;
                    let question: Option<String> = col(r, 6, "Q3")?;
                    Some(T3RequestItem::Question(question.unwrap_or_default()))
                }
                Some(_) => Some(T3RequestItem::Approval(col(r, 8, "Q3")?)),
            };
            Ok(T3Request {
                thread_id: col(r, 1, "Q3")?,
                root: col(r, 2, "Q3")?,
                kind,
                created_at: time(&col::<String>(r, 4, "Q3")?, &format!("request {id:?}"))?,
                item,
                id,
            })
        })?;
        // A request with two items is listed once.
        Ok(all
            .into_iter()
            .filter(|q| seen.insert(q.id.clone()))
            .collect())
    }

    fn q4(&self, conn: &Connection, thread: &str, run: &str) -> Result<Option<String>, T3Error> {
        let found = rows(conn, &self.q4, params![thread, run], "Q4 (failure)", |r| {
            require(
                col(r, 1, "Q4")?,
                "$.failure.message",
                "error item of run",
                run,
            )?;
            col::<Option<String>>(r, 0, "Q4")
        })?;
        Ok(found.into_iter().next().flatten())
    }

    fn q5(&self, conn: &Connection, linked: &str) -> Result<HashMap<String, u32>, T3Error> {
        Ok(rows(conn, &self.q5, params![linked], "Q5 (children)", |r| {
            let n: i64 = col(r, 1, "Q5")?;
            Ok((col::<String>(r, 0, "Q5")?, u32::try_from(n).unwrap_or(0)))
        })?
        .into_iter()
        .collect())
    }

    fn q6(&self, conn: &Connection, since: &str) -> Result<Vec<T3Untracked>, T3Error> {
        rows(conn, &self.q6, params![since], "Q6 (untracked)", |r| {
            let id: String = col(r, 0, "Q6")?;
            let what = format!("thread {id:?}");
            let settled: Option<String> = col(r, 4, "Q6")?;
            Ok(T3Untracked {
                title: col(r, 1, "Q6")?,
                updated_at: time(&col::<String>(r, 3, "Q6")?, &what)?,
                settled: settled.is_some(),
                branch: col(r, 5, "Q6")?,
                project_path: col(r, 6, "Q6")?,
                id,
            })
        })
    }

    fn q8(&self, conn: &Connection) -> Result<Vec<T3Project>, T3Error> {
        rows(conn, &self.q8, [], "Q8 (projects)", |r| {
            Ok(T3Project {
                id: col(r, 0, "Q8")?,
                name: col(r, 1, "Q8")?,
                path: PathBuf::from(col::<String>(r, 2, "Q8")?),
            })
        })
    }
}

/// The deepest project whose root holds `project`, comparing canonical paths.
fn project_for<'a>(projects: &'a [T3Project], project: &Path) -> Option<&'a T3Project> {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let wanted = canonical(project);
    projects
        .iter()
        .map(|p| (p, canonical(&p.path)))
        .filter(|(_, root)| wanted.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())
        .map(|(p, _)| p)
}

/// A UUID: 8-4-4-4-12 hex digits.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

impl T3Source for T3Db {
    fn open(&mut self) -> Result<(), T3Error> {
        self.conn = None;
        let path = T3Db::database_path(&self.dir);
        if !path.is_file() {
            return Err(T3Error::Missing);
        }
        let conn = Connection::open_with_flags(
            uri(&path),
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| sql_error("opening T3's database", e))?;
        conn.pragma_update(None, "query_only", 1)
            .map_err(|e| sql_error("query_only", e))?;
        conn.busy_timeout(Duration::from_millis(200))
            .map_err(|e| sql_error("busy_timeout", e))?;
        conn.pragma_update(None, "cache_size", -2000)
            .map_err(|e| sql_error("cache_size", e))?;
        check(&conn)?;
        self.conn = Some(conn);
        Ok(())
    }

    fn data_version(&mut self) -> Result<i64, T3Error> {
        self.parts()?
            .0
            .pragma_query_value(None, "data_version", |r| r.get(0))
            .map_err(|e| sql_error("data_version", e))
    }

    fn read(&mut self, q: &T3Query) -> Result<T3Read, T3Error> {
        let linked = json_list(&q.linked);
        let (conn, queries) = self.parts()?;
        // One short read transaction, so every query sees the same moment.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| sql_error("BEGIN", e))?;
        let mut read = T3Read {
            threads: queries.q1(&tx, &linked)?,
            runs: queries.q2(&tx, &linked)?,
            requests: queries.q3(&tx)?,
            children: queries.q5(&tx, &linked)?,
            ..T3Read::default()
        };
        for run in &read.runs {
            if run.status == RunStatus::Failed && !q.known_failures.contains(&run.run_id) {
                let message = queries.q4(&tx, &run.thread_id, &run.run_id)?;
                read.failures.insert(run.run_id.clone(), message);
            }
        }
        if let Some(since) = &q.untracked_since {
            read.untracked = Some(queries.q6(&tx, since)?);
            read.projects = Some(queries.q8(&tx)?);
        }
        tx.commit().map_err(|e| sql_error("COMMIT", e))?;
        Ok(read)
    }

    fn find_thread(&mut self, project: &Path, branch: &str) -> Result<Option<String>, T3Error> {
        let (conn, queries) = self.parts()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| sql_error("BEGIN", e))?;
        let projects = queries.q8(&tx)?;
        let Some(found) = project_for(&projects, project) else {
            return Ok(None);
        };
        let id = tx
            .prepare_cached(&queries.q7)
            .and_then(|mut s| {
                s.query_row(params![found.id, branch], |r| r.get::<_, String>(0))
                    .optional()
            })
            .map_err(|e| sql_error("Q7 (find thread)", e))?;
        tx.commit().map_err(|e| sql_error("COMMIT", e))?;
        Ok(id)
    }

    fn environment_id(&mut self) -> Result<Option<String>, T3Error> {
        Ok(std::fs::read_to_string(self.dir.join("environment-id"))
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| is_uuid(t)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_reads_are_guarded() {
        assert_eq!(
            j("t", "$.branch"),
            "CASE WHEN json_valid(t.payload_json) THEN json_extract(t.payload_json, '$.branch') END"
        );
        assert!(jt("r", "$.startedAt").contains("json_type(r.payload_json, '$.startedAt')"));
    }

    #[test]
    fn uris_escape_what_sqlite_reads_as_syntax() {
        assert_eq!(
            uri(Path::new("/a b/50%?#/statev2.sqlite")),
            "file:/a b/50%25%3f%23/statev2.sqlite?mode=ro"
        );
    }

    #[test]
    fn uuids() {
        assert!(is_uuid("0b8a1f9e-1d2c-4b3a-9f8e-7d6c5b4a3f21"));
        assert!(!is_uuid("0b8a1f9e-1d2c-4b3a-9f8e"));
        assert!(!is_uuid("0b8a1f9e-1d2c-4b3a-9f8e-7d6c5b4a3fzz"));
        assert!(!is_uuid(""));
    }

    #[test]
    fn the_deepest_project_wins() {
        let p = |id: &str, path: &str| T3Project {
            id: id.into(),
            name: id.into(),
            path: path.into(),
        };
        let projects = vec![p("web", "/work/web"), p("ui", "/work/web/packages/ui")];
        let find = |path: &str| project_for(&projects, Path::new(path)).map(|p| p.id.as_str());
        assert_eq!(find("/work/web/packages/ui/src"), Some("ui"));
        assert_eq!(find("/work/web/src"), Some("web"));
        assert_eq!(
            find("/work/webby"),
            None,
            "a prefix of the text isn't a parent"
        );
        assert_eq!(find("/elsewhere"), None);
    }
}
