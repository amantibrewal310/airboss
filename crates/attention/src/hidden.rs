//! `attention_hidden`: one row per dismissed or snoozed item.

use std::path::Path;
use std::time::SystemTime;

use airboss_core::TaskId;
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{ErrorCode, NeedsKey, NeedsKind, ServiceError};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{db, log};

/// One dismissed (`until` is `None`) or snoozed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenRow {
    pub key: NeedsKey,
    pub kind: NeedsKind,
    pub task: TaskId,
    pub until: Option<SystemTime>,
}

/// The worker's own connection to `attention_hidden`.
///
/// When the table can't be opened it is *unavailable*: it hides nothing (showing too much
/// beats hiding a question) and every write is a `Storage` error, so the item stays.
pub struct HiddenStore {
    conn: Option<Connection>,
}

impl std::fmt::Debug for HiddenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiddenStore")
            .field("available", &self.conn.is_some())
            .finish()
    }
}

impl HiddenStore {
    /// The table in the database file at `path` (the app's `Store::path()`), migrated first.
    pub fn open(path: &Path) -> Result<HiddenStore, ServiceError> {
        db::open_file(path)
            .map(|conn| HiddenStore { conn: Some(conn) })
            .map_err(|e| storage(&e))
    }

    /// A private in-memory table that forgets everything when dropped.
    pub fn in_memory() -> Result<HiddenStore, ServiceError> {
        db::open_memory()
            .map(|conn| HiddenStore { conn: Some(conn) })
            .map_err(|e| storage(&e))
    }

    /// A table that couldn't be opened.
    pub fn unavailable() -> HiddenStore {
        HiddenStore { conn: None }
    }

    /// Every row. A snooze whose end can't be read counts as ended.
    pub fn load(&self) -> Result<Vec<HiddenRow>, ServiceError> {
        let Some(conn) = &self.conn else {
            return Ok(Vec::new());
        };
        let read = || -> rusqlite::Result<Vec<HiddenRow>> {
            let mut stmt =
                conn.prepare("SELECT item_key, kind, task_id, until FROM attention_hidden")?;
            let rows = stmt.query_map([], |r| {
                let kind: String = r.get(1)?;
                let until: Option<String> = r.get(3)?;
                Ok((r.get::<_, String>(0)?, kind, r.get::<_, i64>(2)?, until))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (key, kind, task, until) = row?;
                // The CHECK allows only known kinds; anything else is a hand-edited file, or a
                // "finish" row from before parents became containers (SPEC-core rule 10).
                let Some(kind) = kind_from(&kind) else {
                    continue;
                };
                out.push(HiddenRow {
                    key: NeedsKey(key),
                    kind,
                    task: TaskId(task),
                    until: until.map(|u| from_ts(&u).unwrap_or(SystemTime::UNIX_EPOCH)),
                });
            }
            Ok(out)
        };
        read().map_err(|e| storage(&e.to_string()))
    }

    /// Hides `row.key`, replacing any row it had.
    pub fn hide(&self, row: &HiddenRow) -> Result<(), ServiceError> {
        let conn = self.conn()?;
        let (action, until) = match row.until {
            Some(t) => ("snoozed", Some(to_ts(t))),
            None => ("dismissed", None),
        };
        conn.execute(
            "INSERT INTO attention_hidden (item_key, task_id, kind, action, until)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (item_key) DO UPDATE SET
               task_id = excluded.task_id, kind = excluded.kind, action = excluded.action,
               until = excluded.until, hidden_at = excluded.hidden_at",
            params![row.key.0, row.task.0, kind_text(row.kind), action, until],
        )
        .map(|_| ())
        .map_err(|e| storage(&e.to_string()))
    }

    /// Deletes `key`'s row; `false` if there was none.
    pub fn restore(&self, key: &NeedsKey) -> Result<bool, ServiceError> {
        self.conn()?
            .execute(
                "DELETE FROM attention_hidden WHERE item_key = ?1",
                params![key.0],
            )
            .map(|n| n > 0)
            .map_err(|e| storage(&e.to_string()))
    }

    /// Deletes these keys' rows, in one transaction.
    pub fn prune(&self, keys: &[NeedsKey]) -> Result<(), ServiceError> {
        let conn = self.conn()?;
        let run = || -> rusqlite::Result<()> {
            let tx = conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare("DELETE FROM attention_hidden WHERE item_key = ?1")?;
                for k in keys {
                    stmt.execute(params![k.0])?;
                }
            }
            tx.commit()
        };
        run().map_err(|e| storage(&e.to_string()))
    }

    /// Whether `key` has a row.
    pub fn contains(&self, key: &NeedsKey) -> Result<bool, ServiceError> {
        self.conn()?
            .query_row(
                "SELECT 1 FROM attention_hidden WHERE item_key = ?1",
                params![key.0],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(|e| storage(&e.to_string()))
    }

    fn conn(&self) -> Result<&Connection, ServiceError> {
        self.conn
            .as_ref()
            .ok_or_else(|| storage("the attention table isn't open"))
    }
}

/// A `Storage` error for the user, with the detail in the log.
fn storage(detail: &str) -> ServiceError {
    log(&format!("hidden items: {detail}"));
    ServiceError::new(
        ErrorCode::Storage,
        "Couldn't save that, so Needs me is unchanged.",
    )
}

fn kind_text(kind: NeedsKind) -> &'static str {
    match kind {
        NeedsKind::Question => "question",
        NeedsKind::Approval => "approval",
        NeedsKind::Review => "review",
        NeedsKind::FailedRun => "failed_run",
        NeedsKind::AgentStopped => "stopped",
        NeedsKind::Proposed => "proposed",
        NeedsKind::Ready => "ready",
        NeedsKind::Quiet => "quiet",
    }
}

fn kind_from(text: &str) -> Option<NeedsKind> {
    Some(match text {
        "question" => NeedsKind::Question,
        "approval" => NeedsKind::Approval,
        "review" => NeedsKind::Review,
        "failed_run" => NeedsKind::FailedRun,
        "stopped" => NeedsKind::AgentStopped,
        "proposed" => NeedsKind::Proposed,
        "ready" => NeedsKind::Ready,
        "quiet" => NeedsKind::Quiet,
        _ => return None,
    })
}
