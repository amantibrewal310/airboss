-- A small T3 world on top of t3_schema.sql: one thread for each state sync tells apart.
-- Every value is made up. Tests run at 2026-11-26T12:00:00Z with a 7-day untracked window.
-- Thread payloads carry every key T3 0.1.12 always writes, and only those sync reads.

INSERT INTO projection_projects (project_id, title, workspace_root, scripts_json, created_at, updated_at, deleted_at, archived_at) VALUES
  ('p-web',  'web',  '/work/web',              '[]', '2026-11-01T00:00:00.000Z', '2026-11-25T00:00:00.000Z', NULL, NULL),
  ('p-ui',   'ui',   '/work/web/packages/ui',  '[]', '2026-11-01T00:00:00.000Z', '2026-11-24T00:00:00.000Z', NULL, NULL),
  ('p-api',  'api',  '/work/api',              '[]', '2026-11-01T00:00:00.000Z', '2026-11-23T00:00:00.000Z', NULL, NULL),
  ('p-gone', 'gone', '/work/gone',             '[]', '2026-11-01T00:00:00.000Z', '2026-11-26T00:00:00.000Z', '2026-11-02T00:00:00.000Z', NULL),
  ('p-old',  'old',  '/work/old',              '[]', '2026-11-01T00:00:00.000Z', '2026-11-26T00:00:00.000Z', NULL, '2026-11-02T00:00:00.000Z');

-- One row per thread: id, project, title, created, updated, archived, deleted, branch,
-- worktree, parent, root, settledAt, pullRequests (absent when NULL), sideChat (absent when NULL).
CREATE TEMP TABLE t (id, project, title, created, updated, archived, deleted, branch, worktree, parent, root, settled, prs, side);
INSERT INTO t VALUES
  ('t-run',      'p-web', 'Running thread',   '2026-11-26T08:00:00.000Z', '2026-11-26T11:00:00.000Z', NULL, NULL, 'feat/run', '/work/wt/run', NULL, 't-run', NULL, '[]', NULL),
  ('t-queued',   'p-web', 'Queued thread',    '2026-11-26T08:00:00.000Z', '2026-11-26T10:00:00.000Z', NULL, NULL, 'feat/queued', NULL, NULL, 't-queued', NULL, '[]', NULL),
  ('t-question', 'p-web', 'Asks a question',  '2026-11-26T08:00:00.000Z', '2026-11-26T11:30:00.000Z', NULL, NULL, 'feat/ask', '/work/wt/ask', NULL, 't-question', NULL, '[]', NULL),
  ('t-approve',  'p-api', 'Wants approval',   '2026-11-26T08:00:00.000Z', '2026-11-26T11:20:00.000Z', NULL, NULL, 'feat/approve', NULL, NULL, 't-approve', NULL, '[]', NULL),
  ('t-approve2', 'p-api', 'Approval, no prompt', '2026-11-26T08:00:00.000Z', '2026-11-26T11:10:00.000Z', NULL, NULL, 'feat/approve2', NULL, NULL, 't-approve2', NULL, '[]', NULL),
  ('t-root',     'p-web', 'Root with a subagent', '2026-11-26T07:00:00.000Z', '2026-11-26T09:00:00.000Z', NULL, NULL, 'feat/root', NULL, NULL, 't-root', NULL, '[]', NULL),
  ('t-child',    'p-web', 'Subagent',         '2026-11-26T07:30:00.000Z', '2026-11-26T09:30:00.000Z', NULL, NULL, 'feat/root', NULL, 't-root', 't-root', NULL, '[]', NULL),
  ('t-child2',   'p-web', 'Second subagent',  '2026-11-26T07:40:00.000Z', '2026-11-26T09:40:00.000Z', NULL, NULL, 'feat/root', NULL, 't-root', 't-root', NULL, NULL, NULL),
  ('t-settled',  'p-web', 'Settled thread',   '2026-11-25T08:00:00.000Z', '2026-11-25T12:00:00.000Z', NULL, NULL, 'feat/settled', NULL, NULL, 't-settled', '2026-11-25T12:00:00.000Z', '[]', NULL),
  ('t-errored',  'p-api', 'Failed run',       '2026-11-26T06:00:00.000Z', '2026-11-26T06:30:00.000Z', NULL, NULL, 'feat/err', NULL, NULL, 't-errored', NULL, '[]', NULL),
  ('t-failed',   'p-api', 'Failed, no item',  '2026-11-26T06:00:00.000Z', '2026-11-26T06:40:00.000Z', NULL, NULL, 'feat/fail', NULL, NULL, 't-failed', NULL, '[]', NULL),
  ('t-idle',     'p-api', 'Idle thread',      '2026-11-26T05:00:00.000Z', '2026-11-26T05:30:00.000Z', NULL, NULL, NULL, NULL, NULL, 't-idle', NULL, NULL, NULL),
  ('t-noruns',   'p-api', 'No runs yet',      '2026-11-26T04:00:00.000Z', '2026-11-26T04:00:00.000Z', NULL, NULL, 'feat/new', NULL, NULL, 't-noruns', NULL, '[]', NULL),
  ('t-archived', 'p-web', 'Archived',         '2026-11-20T00:00:00.000Z', '2026-11-21T00:00:00.000Z', '2026-11-21T00:00:00.000Z', NULL, 'feat/arch', NULL, NULL, 't-archived', NULL, '[]', NULL),
  ('t-deleted',  'p-web', 'Deleted',          '2026-11-20T00:00:00.000Z', '2026-11-21T00:00:00.000Z', NULL, '2026-11-21T00:00:00.000Z', 'feat/run', NULL, NULL, 't-deleted', NULL, '[]', NULL),
  ('t-prs',      'p-web', 'Lists PRs',        '2026-11-26T09:00:00.000Z', '2026-11-26T10:30:00.000Z', NULL, NULL, 'feat/prs', NULL, NULL, 't-prs', NULL,
     '[{"host":"github.com","repository":"Acme/Web","number":7,"url":"https://github.com/Acme/Web/pull/7","source":"agent","linkedAt":"2026-11-26T10:00:00.000Z","snapshot":{"state":"merged"},"stack":null},
       {"host":"github.com","repository":"acme/web","number":3,"url":"https://github.com/acme/web/pull/3","source":"agent","linkedAt":"2026-11-20T10:00:00.000Z","snapshot":null,"stack":null},
       {"host":"gitlab.com","repository":"acme/web","number":9,"url":"https://gitlab.com/acme/web/-/merge_requests/9","source":"agent","linkedAt":"2026-11-26T10:00:00.000Z","snapshot":null,"stack":null}]', NULL),
  ('t-side',     'p-web', 'Side chat',        '2026-11-26T09:00:00.000Z', '2026-11-26T09:10:00.000Z', NULL, NULL, NULL, NULL, NULL, 't-side', NULL, '[]', 1),
  ('t-old',      'p-web', 'Old thread',       '2026-10-01T00:00:00.000Z', '2026-10-02T00:00:00.000Z', NULL, NULL, 'feat/old', NULL, NULL, 't-old', '2026-10-02T00:00:00.000Z', '[]', NULL),
  ('t-web-new',  'p-web', 'Newer on feat/run', '2026-11-26T09:30:00.000Z', '2026-11-26T09:30:00.000Z', NULL, NULL, 'feat/run', NULL, NULL, 't-web-new', NULL, '[]', NULL),
  ('t-ui',       'p-ui',  'In the ui package', '2026-11-26T09:00:00.000Z', '2026-11-26T09:00:00.000Z', NULL, NULL, 'feat/run', NULL, NULL, 't-ui', NULL, '[]', NULL);

INSERT INTO orchestration_v2_projection_threads
  (thread_id, project_id, title, default_provider, runtime_mode, interaction_mode, created_at, updated_at, archived_at, deleted_at, payload_json)
SELECT id, project, title, 'claudeAgent', 'full-access', 'default', created, updated, archived, deleted,
  json_patch(
    json_object('id', id, 'projectId', project, 'title', title, 'branch', branch, 'worktreePath', worktree,
      'lineage', json_patch(json_object('parentThreadId', parent, 'relationshipToParent', CASE WHEN parent IS NULL THEN NULL ELSE 'subagent' END, 'rootThreadId', root),
                            CASE WHEN side IS NULL THEN '{}' ELSE json_object('sideChat', json('true')) END),
      'settledAt', settled, 'createdAt', created, 'updatedAt', updated, 'archivedAt', archived, 'deletedAt', deleted),
    CASE WHEN prs IS NULL THEN '{}' ELSE json_object('pullRequests', json(prs)) END)
FROM t;
DROP TABLE t;

-- json_patch drops keys whose value is null, so put the nullable required keys back.
UPDATE orchestration_v2_projection_threads SET payload_json = json_set(payload_json,
  '$.branch', json_extract(payload_json, '$.branch'),
  '$.worktreePath', json_extract(payload_json, '$.worktreePath'),
  '$.settledAt', json_extract(payload_json, '$.settledAt'),
  '$.lineage.parentThreadId', json_extract(payload_json, '$.lineage.parentThreadId'));

-- Runs: id, thread, ordinal, status, requested, started, completed. Earlier runs are there
-- to prove only the latest counts.
CREATE TEMP TABLE r (id, thread, ordinal, status, requested, started, completed);
INSERT INTO r VALUES
  ('r-run-1',      't-run',      1, 'failed',    '2026-11-26T08:00:00.000Z', '2026-11-26T08:00:01.000Z', '2026-11-26T08:10:00.000Z'),
  ('r-run-2',      't-run',      2, 'running',   '2026-11-26T10:59:00.000Z', '2026-11-26T11:00:00.000Z', NULL),
  ('r-queued-1',   't-queued',   1, 'queued',    '2026-11-26T10:00:00.000Z', NULL, NULL),
  ('r-question-1', 't-question', 1, 'waiting',   '2026-11-26T11:00:00.000Z', '2026-11-26T11:00:01.000Z', NULL),
  ('r-approve-1',  't-approve',  1, 'running',   '2026-11-26T11:00:00.000Z', '2026-11-26T11:00:01.000Z', NULL),
  ('r-approve2-1', 't-approve2', 1, 'running',   '2026-11-26T11:00:00.000Z', '2026-11-26T11:00:01.000Z', NULL),
  ('r-root-1',     't-root',     1, 'completed', '2026-11-26T07:00:00.000Z', '2026-11-26T07:00:01.000Z', '2026-11-26T08:00:00.000Z'),
  ('r-child-1',    't-child',    1, 'running',   '2026-11-26T09:00:00.000Z', '2026-11-26T09:00:01.000Z', NULL),
  ('r-settled-1',  't-settled',  1, 'completed', '2026-11-25T08:00:00.000Z', '2026-11-25T08:00:01.000Z', '2026-11-25T11:00:00.000Z'),
  ('r-errored-1',  't-errored',  1, 'completed', '2026-11-26T06:00:00.000Z', '2026-11-26T06:00:01.000Z', '2026-11-26T06:10:00.000Z'),
  ('r-errored-2',  't-errored',  2, 'failed',    '2026-11-26T06:20:00.000Z', '2026-11-26T06:20:01.000Z', '2026-11-26T06:30:00.000Z'),
  ('r-failed-1',   't-failed',   1, 'failed',    '2026-11-26T06:35:00.000Z', NULL, '2026-11-26T06:40:00.000Z'),
  ('r-idle-1',     't-idle',     1, 'interrupted', '2026-11-26T05:00:00.000Z', '2026-11-26T05:00:01.000Z', '2026-11-26T05:30:00.000Z'),
  ('r-prs-1',      't-prs',      1, 'completed', '2026-11-26T09:00:00.000Z', '2026-11-26T09:00:01.000Z', '2026-11-26T10:30:00.000Z');
INSERT INTO orchestration_v2_projection_runs (run_id, thread_id, ordinal, provider, status, requested_at, completed_at, payload_json)
SELECT id, thread, ordinal, 'claudeAgent', status, requested, completed,
  json_object('id', id, 'threadId', thread, 'ordinal', ordinal, 'status', status, 'requestedAt', requested,
              'startedAt', started, 'completedAt', completed)
FROM r;
DROP TABLE r;

-- Requests: pending ones make needs_me; resolved ones must be ignored.
INSERT INTO orchestration_v2_projection_runtime_requests (runtime_request_id, thread_id, node_id, kind, status, created_at, resolved_at, payload_json) VALUES
  ('q-question', 't-question', 'n-question', 'user_input',  'pending',  '2026-11-26T11:30:00.000Z', NULL, '{}'),
  ('q-old',      't-question', 'n-old',      'user_input',  'resolved', '2026-11-26T11:05:00.000Z', '2026-11-26T11:06:00.000Z', '{}'),
  ('q-approve',  't-approve',  'n-approve',  'command',     'pending',  '2026-11-26T11:20:00.000Z', NULL, '{}'),
  ('q-approve2', 't-approve2', 'n-approve2', 'file-change', 'pending',  '2026-11-26T11:10:00.000Z', NULL, '{}'),
  ('q-child',    't-child',    'n-child',    'user_input',  'pending',  '2026-11-26T09:30:00.000Z', NULL, '{}');

-- Turn items: the request items, and the error items of failed runs.
INSERT INTO orchestration_v2_projection_turn_items (turn_item_id, thread_id, run_id, node_id, ordinal, type, status, updated_at, payload_json) VALUES
  ('i-question', 't-question', 'r-question-1', 'n-question', 1, 'user_input_request', 'pending', '2026-11-26T11:30:00.000Z',
     '{"type":"user_input_request","questions":[{"id":"q1","header":"Ship","question":"Ship it to staging now?","options":[],"multiSelect":false}]}'),
  ('i-old',      't-question', 'r-question-1', 'n-old', 0, 'user_input_request', 'completed', '2026-11-26T11:06:00.000Z',
     '{"type":"user_input_request","questions":[{"question":"An old question"}]}'),
  ('i-approve',  't-approve',  'r-approve-1',  'n-approve', 1, 'approval_request', 'pending', '2026-11-26T11:20:00.000Z',
     '{"type":"approval_request","requestKind":"command","prompt":"cargo publish --dry-run","reason":null}'),
  ('i-approve2', 't-approve2', 'r-approve2-1', 'n-approve2', 1, 'approval_request', 'pending', '2026-11-26T11:10:00.000Z',
     '{"type":"approval_request","requestKind":"file-change","reason":null}'),
  ('i-child',    't-child',    'r-child-1',    'n-child', 1, 'user_input_request', 'pending', '2026-11-26T09:30:00.000Z',
     '{"type":"user_input_request","questions":[{"question":"Which schema?"}]}'),
  ('i-err-old',  't-errored',  'r-errored-2',  'n-e1', 1, 'error', 'failed', '2026-11-26T06:25:00.000Z',
     '{"type":"error","failure":{"class":"provider","message":"An earlier failure","code":null,"retryable":true}}'),
  ('i-err',      't-errored',  'r-errored-2',  'n-e2', 2, 'error', 'failed', '2026-11-26T06:30:00.000Z',
     '{"type":"error","failure":{"class":"usage_limit","message":"Usage limit reached. Try again at 18:00.","code":"usage_limit","retryable":false}}'),
  ('i-err-run1', 't-run',      'r-run-1',      'n-e3', 1, 'error', 'failed', '2026-11-26T08:10:00.000Z',
     '{"type":"error","failure":{"class":"provider","message":"Old failure","code":null,"retryable":true}}');
