-- sync v1: what sync last saw. Its own tables; no foreign keys into core's.

CREATE TABLE observed_threads (           -- one T3 thread or Claude session, linked or untracked
  source TEXT NOT NULL CHECK (source IN ('t3','claude')),
  ref    TEXT NOT NULL CHECK (length(ref) > 0),  -- T3 thread id or Claude session id; same text as links.ref
  linked INTEGER NOT NULL CHECK (linked IN (0,1)),
  state  TEXT NOT NULL CHECK (state IN ('running','needs_me','errored','idle','settled','gone')),
  title TEXT, project_path TEXT, branch TEXT, worktree_path TEXT, parent_ref TEXT,
  child_count INTEGER NOT NULL DEFAULT 0 CHECK (child_count >= 0),
  run_id TEXT, error_text TEXT,                  -- latest run; its failure message when errored
  request_id TEXT, request_kind TEXT CHECK (request_kind IN ('question','approval')),
  request_text TEXT, request_at TEXT,
  pr_seen    TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_seen)),     -- T3-listed PRs already handled
  pr_waiting TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(pr_waiting)),  -- T3-listed PRs waiting for a GitHub read
  noted_run_id TEXT,                             -- the failed run sync already wrote a note about
  last_activity_at TEXT,
  first_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  changed_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (source, ref),
  CHECK ((request_id IS NULL) = (request_kind IS NULL)),
  CHECK ((state = 'needs_me') = (request_id IS NOT NULL))
);
CREATE INDEX observed_threads_linked_idx ON observed_threads(linked, last_activity_at);

CREATE TABLE observed_prs (               -- what GitHub last said about one PR; written only after a gh read
  ref    TEXT PRIMARY KEY CHECK (length(ref) > 0),   -- canonical owner/repo#n, as core stores it
  linked INTEGER NOT NULL CHECK (linked IN (0,1)),   -- 0: one of my open PRs, offered as a link
  url TEXT, title TEXT,
  state  TEXT CHECK (state IN ('open','merged','closed')),   -- null only with an error
  draft  INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0,1)),
  review TEXT CHECK (review IN ('approved','changes_requested','review_required','none')),
  checks TEXT CHECK (checks IN ('passing','failing','pending','none')),
  additions INTEGER, deletions INTEGER, head_ref TEXT, author TEXT, gh_updated_at TEXT, merged_at TEXT,
  ready_since TEXT,                        -- first seen open and not draft
  account TEXT,                            -- the gh login that can read it
  error TEXT CHECK (error IN ('not_found','no_access','failed')), error_detail TEXT,
  checked_at TEXT NOT NULL,
  CHECK ((state IS NULL) = (error IS NOT NULL)),
  CHECK (ready_since IS NULL OR (state = 'open' AND draft = 0))
);

CREATE TABLE sync_health (                -- one row per source, and per gh account
  source TEXT NOT NULL CHECK (source IN ('gh','t3','claude')),
  scope  TEXT NOT NULL DEFAULT '',         -- the gh login; '' for t3 and claude
  state  TEXT NOT NULL CHECK (state IN ('ok','off','broken','rate_limited')),
  detail TEXT, since TEXT NOT NULL, last_ok_at TEXT,
  failures   INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0),  -- drives backoff
  next_try_at TEXT,                        -- backoff or rate-limit end
  PRIMARY KEY (source, scope),
  CHECK (state <> 'rate_limited' OR next_try_at IS NOT NULL)
);

CREATE TABLE sync_finish_held (           -- a task whose PRs merged while an agent on it still works
  task_id INTEGER PRIMARY KEY,             -- no foreign key: core owns tasks
  since   TEXT NOT NULL
);
