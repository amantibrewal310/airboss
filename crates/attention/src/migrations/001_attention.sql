-- attention v1: dismissed and snoozed Needs me items. No foreign key: core owns tasks.
CREATE TABLE attention_hidden (           -- one dismissed or snoozed item
  item_key  TEXT PRIMARY KEY CHECK (length(item_key) > 0),
  task_id   INTEGER NOT NULL,             -- no foreign key: core owns tasks
  kind      TEXT NOT NULL CHECK (kind IN ('question','approval','review','failed_run',
                                          'proposed','ready','finish','quiet')),
  action    TEXT NOT NULL CHECK (action IN ('dismissed','snoozed')),
  until     TEXT,                         -- set exactly for snoozes
  hidden_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((action = 'snoozed') = (until IS NOT NULL))
);
CREATE INDEX attention_hidden_task_idx ON attention_hidden(task_id);
