/*
  PROVISIONAL — written by hand from crates/core/src/model.rs and SPEC-app.md.
  To be replaced by the ts-rs output from Rust (`cargo test -p airboss-app --test fixtures`
  writes app/src/lib/types/*.ts). Ids are `number` (`#[ts(type = "number")]`).
  Serde shapes: unit enums are snake_case strings; `Actor` is its text form.
*/

export type TaskId = number;
export type RepoId = number;
export type LinkId = number;
export type EventId = number;

/** UTC text in core's format, e.g. "2026-11-25T17:19:07.841Z". */
export type Timestamp = string;

export type Status = 'proposed' | 'todo' | 'doing' | 'in_review' | 'blocked' | 'done' | 'dropped';

export type Origin = 'me' | 'agent' | 'plan';

/** "me", "agent:<thread ref>", "sync:gh", "sync:t3", "plan" or "launch". */
export type Actor = string;

export type EventKind =
  | 'created'
  | 'edited'
  | 'moved'
  | 'status'
  | 'note'
  | 'linked'
  | 'unlinked'
  | 'link_state'
  | 'waits_on_added'
  | 'waits_on_removed'
  | 'rejected'
  | 'repo_linked'
  | 'repo_unlinked';

export type LinkKind = 't3_thread' | 'claude_session' | 'pr' | 'slack' | 'doc' | 'github_issue';

export type PrState = 'open' | 'merged' | 'closed';

export interface Task {
  id: TaskId;
  parent: TaskId | null;
  repos: RepoId[];
  title: string;
  body: string | null;
  status: Status;
  blocked_reason: string | null;
  position: number;
  origin: Origin;
  plan_key: string | null;
  branch: string | null;
  created_at: Timestamp;
}

export interface Repo {
  id: RepoId;
  name: string;
  path: string;
  remote_url: string | null;
}

export interface Event {
  id: EventId;
  task: TaskId;
  kind: EventKind;
  to_status: Status | null;
  body: string | null;
  actor: Actor;
  at: Timestamp;
}

export interface Link {
  id: LinkId;
  task: TaskId;
  kind: LinkKind;
  ref: string;
  url: string | null;
  pr_state: PrState | null;
  title: string | null;
  linked_by: Actor;
  linked_at: Timestamp;
}

export interface NewLink {
  kind: LinkKind;
  ref: string;
  url: string | null;
  pr_state: PrState | null;
  title: string | null;
}
