-- core migration 6: a task may have a priority, P0 (most urgent) to P3, stored as 0 to 3.
-- Null means none set. ADD COLUMN leaves every existing row null, so nothing is rebuilt.

ALTER TABLE tasks ADD COLUMN priority INTEGER CHECK (priority IN (0, 1, 2, 3));
