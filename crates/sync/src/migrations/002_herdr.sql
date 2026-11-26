-- sync v2: Herdr terminals are a third source of threads (source 'herdr'). SQLite can't widen a
-- CHECK in place, so observed_threads and sync_health are rebuilt the way core's migration 5
-- rebuilt links (https://sqlite.org/lang_altertable.html#otheralter). Nothing references either
-- table: no foreign key, trigger or view. observed_threads' one index is made again below.
--
-- One rule is loosened while the table is rebuilt: a needs_me thread may have no request. A
-- Claude session that's waiting and a Herdr terminal that's blocked both need me, and neither
-- says what for; v1 refused those rows. A request still means needs_me.

CREATE TABLE observed_threads_new (       -- one T3 thread, Claude session or Herdr terminal
  source TEXT NOT NULL CHECK (source IN ('t3','claude','herdr')),
  ref    TEXT NOT NULL CHECK (length(ref) > 0),  -- T3 thread id, Claude session id or Herdr address; same text as links.ref
  linked INTEGER NOT NULL CHECK (linked IN (0,1)),
  state  TEXT NOT NULL CHECK (state IN ('running','needs_me','errored','idle','settled','gone')),
  title TEXT, project_path TEXT, branch TEXT, worktree_path TEXT, parent_ref TEXT,
  child_count INTEGER NOT NULL DEFAULT 0 CHECK (child_count >= 0),
  run_id TEXT, error_text TEXT,
  request_id TEXT, request_kind TEXT CHECK (request_kind IN ('question','approval')),
  request_text TEXT, request_at TEXT,
  pr_seen    TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_seen)),
  pr_waiting TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_waiting)),
  noted_run_id TEXT,
  last_activity_at TEXT,
  first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  changed_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (source, ref),
  CHECK ((request_id IS NULL) = (request_kind IS NULL)),
  CHECK (request_id IS NULL OR state = 'needs_me')
);

INSERT INTO observed_threads_new (source, ref, linked, state, title, project_path, branch,
    worktree_path, parent_ref, child_count, run_id, error_text, request_id, request_kind,
    request_text, request_at, pr_seen, pr_waiting, noted_run_id, last_activity_at,
    first_seen_at, changed_at)
  SELECT source, ref, linked, state, title, project_path, branch, worktree_path, parent_ref,
    child_count, run_id, error_text, request_id, request_kind, request_text, request_at,
    pr_seen, pr_waiting, noted_run_id, last_activity_at, first_seen_at, changed_at
  FROM observed_threads;

DROP TABLE observed_threads;
ALTER TABLE observed_threads_new RENAME TO observed_threads;
CREATE INDEX observed_threads_linked_idx ON observed_threads(linked, last_activity_at);

CREATE TABLE sync_health_new (            -- one row per source, and per gh account
  source TEXT NOT NULL CHECK (source IN ('gh','t3','claude','herdr')),
  scope  TEXT NOT NULL DEFAULT '',         -- the gh login; '' for the others
  state  TEXT NOT NULL CHECK (state IN ('ok','off','broken','rate_limited')),
  detail TEXT, since TEXT NOT NULL, last_ok_at TEXT,
  failures   INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0),
  next_try_at TEXT,
  PRIMARY KEY (source, scope),
  CHECK (state <> 'rate_limited' OR next_try_at IS NOT NULL)
);

INSERT INTO sync_health_new (source, scope, state, detail, since, last_ok_at, failures, next_try_at)
  SELECT source, scope, state, detail, since, last_ok_at, failures, next_try_at FROM sync_health;

DROP TABLE sync_health;
ALTER TABLE sync_health_new RENAME TO sync_health;
