-- launch migration 2: a launch_repos row remembers the path of the repo it was written for.
--
-- Rows are kept by core's repo id. Before core's migration 4, remove_repo could free an id
-- that the next repo added was then given, so a row could pass to a different repo, and Do it
-- would use its base branch and T3 project. launch reads a row only while its repo_path still
-- equals the repo's current path. Rows from before this migration have no path, so they're
-- ignored until launch records the repo again.

ALTER TABLE launch_repos ADD COLUMN repo_path TEXT CHECK (repo_path IS NULL OR length(repo_path) > 0);
