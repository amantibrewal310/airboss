//! Small helpers for the SQL every module runs.

use rusqlite::{Connection, Params, Row};

use crate::Result;

/// `sub(id)`: task `?1` and every task under it. Callers that want the descendants only
/// add `id <> ?1`. `UNION` keeps the walk finite even on a damaged tree.
pub(crate) const SUBTREE_CTE: &str = "WITH RECURSIVE sub(id) AS (
    SELECT ?1
    UNION
    SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
)";

/// Runs `sql` once and maps every row it returns, in order.
pub(crate) fn query_all<T, P: Params>(
    conn: &Connection,
    sql: &str,
    params: P,
    map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, map)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
