-- core migration 5: links may point at a Herdr terminal (kind 'herdr_terminal'). SQLite can't
-- widen a CHECK in place, so links is rebuilt the way 004 rebuilt repos, following SQLite's
-- procedure for other kinds of table schema change
-- (https://sqlite.org/lang_altertable.html#otheralter). The runner in db.rs turns foreign keys
-- off around this file and runs `foreign_key_check` before the commit.
--
-- Nothing references links: no foreign key, trigger or view. Its one index is made again below.

CREATE TABLE links_new (
  id         INTEGER PRIMARY KEY,
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('t3_thread','claude_session','herdr_terminal','pr','slack','doc','github_issue')),
  ref        TEXT NOT NULL CHECK (length(ref) > 0),
  url        TEXT,
  title      TEXT,
  pr_state   TEXT CHECK (pr_state IN ('open','merged','closed')),
  linked_by  TEXT NOT NULL,
  linked_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (task_id, kind, ref),
  CHECK ((kind = 'pr') = (pr_state IS NOT NULL))
);

INSERT INTO links_new (id, task_id, kind, ref, url, title, pr_state, linked_by, linked_at)
  SELECT id, task_id, kind, ref, url, title, pr_state, linked_by, linked_at FROM links;

DROP TABLE links;
ALTER TABLE links_new RENAME TO links;
CREATE INDEX links_kind_ref_idx ON links(kind, ref);
