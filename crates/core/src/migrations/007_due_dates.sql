-- core migration 7: a task may have a due date (rule 36). The calendar date it's due, as
-- 'YYYY-MM-DD' in my own time zone; null means no due date. ADD COLUMN keeps every row as it
-- is, so no rebuild. The CHECK only holds the shape; core checks the date is a real one.
ALTER TABLE tasks ADD COLUMN due_at TEXT
  CHECK (due_at IS NULL OR due_at GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9]');
