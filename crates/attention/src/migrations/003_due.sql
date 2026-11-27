-- attention v3: the Due kind ('due'). SQLite can't change a CHECK in place, so the table is
-- rebuilt with every row kept, as v2 did.
CREATE TABLE attention_hidden_v3 (        -- one dismissed or snoozed item
  item_key  TEXT PRIMARY KEY CHECK (length(item_key) > 0),
  task_id   INTEGER NOT NULL,             -- no foreign key: core owns tasks
  kind      TEXT NOT NULL CHECK (kind IN ('question','approval','review','failed_run','stopped',
                                          'due','proposed','ready','finish','quiet')),
  action    TEXT NOT NULL CHECK (action IN ('dismissed','snoozed')),
  until     TEXT,                         -- set exactly for snoozes
  hidden_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((action = 'snoozed') = (until IS NOT NULL))
);
INSERT INTO attention_hidden_v3 (item_key, task_id, kind, action, until, hidden_at)
  SELECT item_key, task_id, kind, action, until, hidden_at FROM attention_hidden;
DROP TABLE attention_hidden;
ALTER TABLE attention_hidden_v3 RENAME TO attention_hidden;
CREATE INDEX attention_hidden_task_idx ON attention_hidden(task_id);
