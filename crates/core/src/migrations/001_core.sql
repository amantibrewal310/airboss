-- core migration 1. `schema_migrations` itself is created by the runner in db.rs.

CREATE TABLE repos (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  path        TEXT NOT NULL UNIQUE,
  remote_url  TEXT
);

CREATE TABLE tasks (
  id             INTEGER PRIMARY KEY,
  parent_id      INTEGER REFERENCES tasks(id) ON DELETE CASCADE,
  repo_id        INTEGER REFERENCES repos(id),
  title          TEXT NOT NULL CHECK (length(trim(title)) > 0),
  body           TEXT,
  status         TEXT NOT NULL DEFAULT 'todo'
                 CHECK (status IN ('proposed','todo','doing','in_review','blocked','done','dropped')),
  blocked_reason TEXT,
  position       INTEGER NOT NULL CHECK (position >= 1),
  origin         TEXT NOT NULL CHECK (origin IN ('me','agent','plan')),
  plan_key       TEXT,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((status = 'blocked') = (blocked_reason IS NOT NULL)),
  CHECK (parent_id IS NULL OR parent_id <> id),
  CHECK (plan_key IS NULL OR origin = 'plan')
);
CREATE INDEX tasks_parent_position_idx ON tasks(parent_id, position);
CREATE UNIQUE INDEX tasks_plan_key_idx ON tasks(ifnull(parent_id, 0), plan_key) WHERE plan_key IS NOT NULL;

CREATE TABLE task_events (
  id         INTEGER PRIMARY KEY,
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('created','edited','moved','status','note','linked','unlinked','link_state','waits_on_added','waits_on_removed','rejected')),
  to_status  TEXT CHECK (to_status IN ('proposed','todo','doing','in_review','blocked','done','dropped')),
  body       TEXT,
  actor      TEXT NOT NULL,
  at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK ((kind = 'status') = (to_status IS NOT NULL)),
  CHECK (kind <> 'note' OR body IS NOT NULL)
);
CREATE INDEX task_events_task_idx ON task_events(task_id, id);

CREATE TABLE task_deps (
  task_id     INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  waits_on_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  added_by    TEXT NOT NULL,
  added_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (task_id, waits_on_id),
  CHECK (task_id <> waits_on_id)
);
CREATE INDEX task_deps_waits_on_idx ON task_deps(waits_on_id);

CREATE TABLE links (
  id         INTEGER PRIMARY KEY,
  task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('t3_thread','claude_session','pr','slack','doc','github_issue')),
  ref        TEXT NOT NULL CHECK (length(ref) > 0),
  url        TEXT,
  pr_state   TEXT CHECK (pr_state IN ('open','merged','closed')),
  linked_by  TEXT NOT NULL,
  linked_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE (task_id, kind, ref),
  CHECK ((kind = 'pr') = (pr_state IS NOT NULL))
);
CREATE INDEX links_kind_ref_idx ON links(kind, ref);
