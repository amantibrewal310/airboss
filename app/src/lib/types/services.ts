/*
  PROVISIONAL — written by hand from the `crates/services` contract (SPEC-attention.md
  "Shared contract", quoted by SPEC-app.md "Interfaces"). To be replaced by ts-rs output.
  Serde: unit enums are snake_case strings; data enums are externally tagged
  (`{"ready": {"version": "…"}}`); `PrRef`, `NeedsKey` and `Timestamp` are plain strings;
  tuples are arrays.
*/

import type { Repo, RepoId, TaskId, Timestamp, PrState } from './core';

/** One line of the connections popover. Rows with the same `name` merge (worst state wins). */
export interface Connection {
  name: string;
  ok: boolean;
  state: string;
  detail: string;
  fix: string | null;
}

export type ErrorCode =
  // launch
  | 't3_missing'
  | 't3_not_running'
  | 't3_unreachable'
  | 't3_too_old'
  | 'timeout'
  | 'not_a_project'
  | 'repo_missing'
  | 'no_base_branch'
  | 'bad_branch'
  | 'task_closed'
  | 'task_proposed'
  | 'needs_choice'
  | 'waiting'
  | 'already_starting'
  | 'bad_answer'
  | 't3_failed'
  // sync
  | 'gh_missing'
  | 'signed_out'
  | 'no_access'
  | 'pr_not_found'
  | 'bad_pr_ref'
  | 'rate_limited'
  | 'gh_failed'
  | 't3_unavailable'
  // any service
  | 'not_found'
  | 'core'
  | 'storage'
  | 'internal'
  | 'shutting_down';

export interface ServiceError {
  code: ErrorCode;
  message: string;
}

export type Source = 't3' | 'claude';

export interface ThreadKey {
  source: Source;
  ref: string;
}

export type ThreadState = 'running' | 'needs_me' | 'errored' | 'idle' | 'settled' | 'gone';

export type Chip = 'working' | 'waiting_on_you' | 'stopped';

export type RequestKind = 'question' | 'approval';

export interface Request {
  id: string;
  kind: RequestKind;
  text: string;
  at: Timestamp;
}

export interface ThreadView {
  key: ThreadKey;
  linked: boolean;
  state: ThreadState;
  title: string | null;
  request: Request | null;
  error: string | null;
  run_id: string | null;
  project_path: string | null;
  branch: string | null;
  worktree_path: string | null;
  parent_ref: string | null;
  child_count: number;
  last_activity_at: Timestamp | null;
  stale: boolean;
}

/** "owner/repo#n", lowercase. */
export type PrRef = string;

export type Review = 'approved' | 'changes_requested' | 'required' | 'none';
export type Checks = 'passing' | 'failing' | 'pending' | 'none';
export type PrProblem = 'not_found' | 'no_access' | { failed: string };

export interface PrView {
  ref: PrRef;
  linked: boolean;
  url: string | null;
  title: string | null;
  state: PrState | null;
  draft: boolean;
  review: Review;
  checks: Checks;
  additions: number | null;
  deletions: number | null;
  head_ref: string | null;
  author: string | null;
  updated_at: Timestamp | null;
  ready_since: Timestamp | null;
  problem: PrProblem | null;
  stale: boolean;
}

export interface T3Project {
  id: string;
  name: string;
  path: string;
}

export type Availability = 'unknown' | { ready: { version: string } } | { missing: { reason: string } };

export type Mode = 'do_it' | 'plan_it';

export type Place = 'task_repos' | { no_repo: string };

export interface StartedThread {
  key: ThreadKey;
  mode: Mode;
  repo: RepoId | null;
  title: string;
  linked: boolean;
}

export interface Prepared {
  repos: Repo[];
  from_subtasks: boolean;
  needs_choice: boolean;
  choices: Repo[];
  branch: string | null;
  research_dir: string;
  waiting_on: TaskId[];
  started: StartedThread[];
}

export interface LaunchReport {
  started: StartedThread[];
  skipped: RepoId[];
  failed: [RepoId | null, ServiceError][];
  branch: string | null;
  moved_to_doing: boolean;
}

/** e.g. "question:<request id>"; stable across restarts. */
export type NeedsKey = string;

export type NeedsKind =
  | 'question'
  | 'approval'
  | 'review'
  | 'failed_run'
  | 'proposed'
  | 'ready'
  | 'finish_it'
  | 'quiet';

/** Display order. */
export type NeedsGroup = 'questions' | 'reviews' | 'failed_runs' | 'proposed' | 'ready' | 'finish_it' | 'quiet';

export type Target = { thread: ThreadKey } | { pr: { ref: PrRef; url: string | null } } | { task: TaskId };

export interface NeedsItem {
  key: NeedsKey;
  kind: NeedsKind;
  group: NeedsGroup;
  task: TaskId;
  task_title: string;
  since: Timestamp;
  headline: string;
  context: string;
  target: Target;
  /** Proposed only: [task id, title]. */
  proposed: [TaskId, string][];
  stale: boolean;
}

export interface NeedsList {
  items: NeedsItem[];
  count: number;
  checked_at: Timestamp | null;
  version: number;
}
