//! The write transaction every change runs in.

use std::ops::Deref;

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::{EventId, Result};

/// One call's write transaction, plus the ids of the events it wrote.
///
/// Writers take `&mut Tx`, so a write can't happen outside a transaction, and every event
/// is collected for the change notification sent after commit (rule 23).
pub(crate) struct Tx<'a> {
    tx: Transaction<'a>,
    events: Vec<EventId>,
}

impl<'a> Tx<'a> {
    /// Takes the write lock up front (IMMEDIATE). A deferred transaction that reads and then
    /// writes fails with "database is locked" when another handle writes first, and
    /// `busy_timeout` can't help it.
    pub(crate) fn begin(conn: &'a mut Connection) -> Result<Tx<'a>> {
        Ok(Tx {
            tx: conn.transaction_with_behavior(TransactionBehavior::Immediate)?,
            events: Vec::new(),
        })
    }

    pub(crate) fn push_event(&mut self, id: EventId) {
        self.events.push(id);
    }

    /// Commits and hands back the ids of every event written, in order.
    pub(crate) fn commit(self) -> Result<Vec<EventId>> {
        self.tx.commit()?;
        Ok(self.events)
    }
}

impl Deref for Tx<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.tx
    }
}
