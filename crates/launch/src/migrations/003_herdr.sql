-- launch migration 3: a row remembers where it started its thread, T3 Code or Herdr.
--
-- A Herdr row's thread_id is the terminal's address (such as `t4p`), and Do it in Herdr makes
-- its own git worktree, whose folder is kept in worktree_path. Every row from before this
-- migration was a T3 one.

ALTER TABLE launch_threads ADD COLUMN launcher TEXT NOT NULL DEFAULT 't3'
  CHECK (launcher IN ('t3','herdr'));
ALTER TABLE launch_threads ADD COLUMN worktree_path TEXT
  CHECK (worktree_path IS NULL OR length(worktree_path) > 0);
