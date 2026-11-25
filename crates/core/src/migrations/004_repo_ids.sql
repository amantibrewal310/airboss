-- core migration 4: repo ids are never handed out twice. Rebuilds repos with AUTOINCREMENT,
-- the same way 003 rebuilt task_events, following SQLite's procedure for other kinds of table
-- schema change (https://sqlite.org/lang_altertable.html#otheralter). The runner in db.rs
-- does steps 1, 10 and 12 around this file: foreign keys off before the transaction,
-- `foreign_key_check` before the commit, and foreign keys back on after it.
--
-- Why it matters: remove_repo frees an id, and another module (launch's launch_repos) keeps
-- rows by repo id. A new repo handed a removed repo's id would inherit those rows.

-- Steps 3 and 4. repos has no triggers, views or indexes of its own beyond its two UNIQUE
-- autoindexes, which come with the new table. task_repos references it, so the new table
-- is built under another name and renamed to repos (step 7), which leaves task_repos'
-- REFERENCES repos(id) pointing at it. Renaming the old table away instead would rewrite
-- that reference to follow the old table.
CREATE TABLE repos_new (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL UNIQUE,
  path        TEXT NOT NULL UNIQUE,
  remote_url  TEXT
);

-- Step 5: every row, ids included, so every task_repos row still finds its repo. Inserting
-- explicit ids sets sqlite_sequence to max(id), and the rename carries that row. Ids removed
-- before this migration that were higher than every id left can't be protected: nothing
-- recorded them. launch also checks a repo's path before it trusts a row kept by repo id.
INSERT INTO repos_new (id, name, path, remote_url)
  SELECT id, name, path, remote_url FROM repos;

-- Steps 6 and 7. With foreign keys off, dropping repos leaves task_repos as it is.
DROP TABLE repos;
ALTER TABLE repos_new RENAME TO repos;
