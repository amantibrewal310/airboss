//! One consistent read of everything the app and the services redraw from (rule 30).

use crate::model::CoreSnapshot;
use crate::{Result, deps, events, tasks};

impl crate::Store {
    /// The tree, every dependency edge, the ready list and the newest event id, all read in
    /// one read transaction so they describe the same moment even while another process
    /// writes. The tree is built once and the ready list worked out from it.
    pub fn snapshot(&self) -> Result<CoreSnapshot> {
        // Deferred: a read transaction, holding no lock beyond WAL's read snapshot. It only
        // reads, so dropping it at the end (a rollback) is all the cleanup it needs.
        let tx = self.conn.unchecked_transaction()?;
        let (tree, edges) = tasks::tree_and_edges(&tx)?;
        let ready = deps::ready_from(&tx, &tree, &edges)?;
        Ok(CoreSnapshot {
            edges,
            latest_event_id: events::latest_id(&tx)?,
            ready,
            tree,
        })
    }
}
