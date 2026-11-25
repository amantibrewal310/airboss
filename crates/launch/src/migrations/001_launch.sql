-- launch's own tables (SPEC-launch, Data / SQL). Not shipped yet, so still editable.

CREATE TABLE launch_threads (             -- one thread launch started, or may have started
  id            INTEGER PRIMARY KEY,
  task_id       INTEGER NOT NULL,         -- no foreign key: core owns tasks
  repo_id       INTEGER,                  -- null: research only, or Plan it with no repo
  mode          TEXT NOT NULL CHECK (mode IN ('do','plan')),
  state         TEXT NOT NULL CHECK (state IN ('pending','started','failed','lost')),
  thread_id     TEXT UNIQUE,              -- from `t3 thread start --json` or find_thread
  project_path  TEXT NOT NULL,            -- the folder passed to t3
  branch        TEXT,
  base_ref      TEXT,
  title         TEXT NOT NULL,
  linked        INTEGER NOT NULL DEFAULT 0 CHECK (linked IN (0,1)),   -- 1 once core has the link
  error         TEXT,                     -- why it failed
  created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  settled_at    TEXT,
  CHECK ((state = 'started') = (thread_id IS NOT NULL)),
  CHECK (linked = 0 OR state = 'started'),
  CHECK ((state = 'failed') = (error IS NOT NULL)),
  CHECK ((branch IS NULL) = (base_ref IS NULL)),
  CHECK (branch IS NULL OR (mode = 'do' AND repo_id IS NOT NULL))
);
CREATE INDEX launch_threads_task_idx ON launch_threads(task_id, repo_id, state);
CREATE UNIQUE INDEX launch_threads_one_pending_idx
  ON launch_threads(task_id, ifnull(repo_id, 0), mode) WHERE state = 'pending';

CREATE TABLE launch_repos (               -- per-repo launch settings
  repo_id       INTEGER PRIMARY KEY,
  base_ref      TEXT CHECK (base_ref IS NULL OR length(base_ref) > 0),  -- null: use origin/HEAD
  t3_project_id TEXT,                     -- the project T3 last resolved this repo to
  updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
