-- core migration 3: event ids are never handed out twice (rule 23). Rebuilds task_events with
-- AUTOINCREMENT, following SQLite's procedure for other kinds of table schema change
-- (https://sqlite.org/lang_altertable.html#otheralter). The runner in db.rs does steps 1, 10
-- and 12 around this file: foreign keys off before the transaction, `foreign_key_check`
-- before the commit, and foreign keys back on after it.

-- Step 4: the new table, exactly as before apart from AUTOINCREMENT.
CREATE TABLE task_events_new (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('created','edited','moved','status','note','linked','unlinked','link_state','waits_on_added','waits_on_removed','rejected','repo_linked','repo_unlinked')),
  to_status  TEXT CHECK (to_status IN ('proposed','todo','doing','in_review','blocked','done','dropped')),
  body       TEXT,
  actor      TEXT NOT NULL,
  at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((kind = 'status') = (to_status IS NOT NULL)),
  CHECK (kind <> 'note' OR body IS NOT NULL)
);

-- Step 5: every row, ids included. Inserting explicit ids sets sqlite_sequence to max(id),
-- so no id already in the table can be handed out again. Ids deleted before this migration
-- above that max left no trace, so nothing can protect those.
INSERT INTO task_events_new (id, task_id, kind, to_status, body, actor, at)
  SELECT id, task_id, kind, to_status, body, actor, at FROM task_events;

-- Steps 6 and 7. Nothing references task_events, and the rename carries sqlite_sequence's row.
DROP TABLE task_events;
ALTER TABLE task_events_new RENAME TO task_events;

-- Step 8: the index went with the old table. No triggers or views use it (step 9).
CREATE INDEX task_events_task_idx ON task_events(task_id, id);
