-- core migration 2: PR refs in one form, lowercase owner/repo#n (rule 29). No table changes.
-- Refs stored as URLs were never valid, so they're left as they are.

UPDATE OR IGNORE links SET ref = lower(ref)
WHERE kind = 'pr' AND ref <> lower(ref) AND instr(ref, '://') = 0;

-- Where a task already had the lowercase twin, the update above was skipped: the twin stays,
-- with its own state, and the old spelling goes.
DELETE FROM links
WHERE kind = 'pr' AND ref <> lower(ref) AND instr(ref, '://') = 0
  AND EXISTS (SELECT 1 FROM links twin
              WHERE twin.task_id = links.task_id AND twin.kind = 'pr' AND twin.ref = lower(links.ref));
