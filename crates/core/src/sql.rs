//! Small helpers for the SQL every module runs.

use rusqlite::{Connection, Params, Row};

use crate::Result;

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
