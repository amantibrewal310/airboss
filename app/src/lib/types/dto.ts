/*
  PROVISIONAL — written by hand from SPEC-app.md (Architecture, Appendix A and B).
  These mirror `app/src-tauri/src/dto.rs`, which doesn't exist yet. To be replaced by the
  ts-rs output from Rust. Fields marked EXTRA are ones the UI needs beyond the spec's text;
  they are listed in the build report so the backend can add them.
*/

import type { Actor, Event, Link, Repo, RepoId, Status, Origin, Task, TaskId, Timestamp, LinkId } from './core';
import type {
  Availability,
  Chip,
  Connection,
  ErrorCode,
  LaunchReport,
  NeedsItem,
  NeedsKey,
  NeedsList,
  Prepared,
  PrView,
  Request,
  ServiceError,
  ThreadKey,
  ThreadState,
} from './services';

/** Leaf subtasks at any depth; dropped left out, proposed counted as todo. */
export interface Progress {
  done: number;
  total: number;
  in_review: number;
  doing: number;
  blocked: number;
}

/** One task as the list draws it. Rust works out every field; the UI never computes tree facts. */
export interface RowDto {
  id: TaskId;
  parent: TaskId | null;
  depth: number;
  position: number;
  title: string;
  status: Status;
  origin: Origin;
  blocked_reason: string | null;
  /** A todo task with an open blocker (drawn as the dashed "waiting" ring). */
  waiting: boolean;
  /** Open blockers, only while waiting. */
  waits_on: TaskId[];
  has_children: boolean;
  progress: Progress | null;
  repos: RepoId[];
  repos_from_subtasks: boolean;
  branch: string | null;
  ready: boolean;
  ready_since: Timestamp | null;
  ready_pill: boolean;
  quiet_days: number | null;
  needs: boolean;
  needs_stale: boolean;
  live: Chip | null;
  last_at: Timestamp;
  last_actor: Actor;
}

export interface Patch {
  seq: number;
  upserts: RowDto[];
  removed: TaskId[];
  /** Ids in tree order; sent only when it changed. */
  order: TaskId[] | null;
  /** Sent only when it changed. */
  repos: Repo[] | null;
  /** Drives the new-row highlight. */
  created: TaskId[];
}

export type Appearance = 'system' | 'dark' | 'light';

export interface Settings {
  /** An accelerator in Tauri's global-shortcut syntax, e.g. "Control+Alt+N". */
  hotkey: string;
  open_at_login: boolean;
  appearance: Appearance;
  folded: TaskId[];
}

/** EXTRA: whether the `airboss` command-line tool is linked, for Settings and the Agents row. */
export interface CliStatus {
  installed: boolean;
  path: string;
  on_path: boolean;
}

export interface Bootstrap {
  seq: number;
  rows: RowDto[];
  order: TaskId[];
  repos: Repo[];
  needs: NeedsList;
  connections: Connection[];
  availability: Availability;
  settings: Settings;
  /** The system accent as "#rrggbb", or null for Multicolor. */
  accent: string | null;
  /** EXTRA */
  cli: CliStatus;
}

/** What the app's own commands return on failure. A `ServiceError` passes through with its code. */
export type AppErrorCode =
  | 'empty_title'
  | 'not_found'
  | 'invalid_target'
  | 'loop'
  | 'already_exists'
  | 'not_allowed'
  | 'invalid_transition'
  | 'db'
  | 'already_starting'
  | 'undo_refused'
  | 'internal'
  /** EXTRA: `bootstrap` when the database won't open; `detail` is the path. */
  | 'db_unavailable'
  | ErrorCode;

export interface AppError {
  code: AppErrorCode;
  message: string;
  detail: string | null;
}

/** EXTRA: an undoable write says which undo entry it made, so its toast can undo exactly that. */
export interface UndoEntry {
  id: number;
  label: string;
}

export interface UndoableWrite {
  patch: Patch | null;
  undo: UndoEntry | null;
}

export interface UndoResult {
  patch: Patch | null;
  message: string;
}

export interface ThreadDto {
  key: ThreadKey;
  state: ThreadState;
  chip: Chip | null;
  title: string | null;
  request: Request | null;
  error: string | null;
  project_path: string | null;
  /** `sync.thread_url(key)`; null for Claude Code sessions, which show their folder instead. */
  url: string | null;
  last_activity_at: Timestamp | null;
  stale: boolean;
}

export interface LinkDto extends Link {
  pr: PrView | null;
  thread: ThreadDto | null;
}

export interface HistoryEntry extends Event {
  /** EXTRA: for "Agent in <thread title>". */
  thread_title: string | null;
}

export interface DepDto {
  id: TaskId;
  title: string;
  status: Status;
  waiting: boolean;
}

export interface RunEdge {
  /** The blocker. */
  from: TaskId;
  /** The task that waits. */
  to: TaskId;
  /** The blocker is done. */
  done: boolean;
}

export interface OutsideWait {
  task: TaskId;
  blockers: TaskId[];
}

/** Display waves (done subtasks included), lifted edges, outside blockers and loops. */
export interface RunMap {
  nodes: TaskId[];
  edges: RunEdge[];
  waves: TaskId[][];
  outside: OutsideWait[];
  tangled: TaskId[];
}

export interface TaskDetail {
  req: number;
  task: Task;
  links: LinkDto[];
  history: HistoryEntry[];
  /** EXTRA: more history exists beyond the limit. */
  history_more: boolean;
  waits_on: DepDto[];
  blocks: DepDto[];
  waves: RunMap | null;
  needs: NeedsItem[];
  /** `launch.prepare(id, DoIt)`; null for proposed, done and dropped tasks. */
  prepared: Prepared | null;
  /** EXTRA: `sync.snapshot().suggested_prs(branch)` for the ⌘L popover. */
  suggested_prs: PrView[];
}

export interface PlanCard {
  id: TaskId;
  title: string;
  status: Status;
  waiting: boolean;
  progress: Progress;
  waves: number;
  /** 0-based; null when every wave is done. */
  current_wave: number | null;
  next_up: { id: TaskId; title: string }[];
  finish_it: boolean;
  working: number;
  blocked: number;
  repos: RepoId[];
  last_at: Timestamp;
}

export interface ParentOption {
  id: TaskId;
  title: string;
  status: Status;
  waiting: boolean;
  depth: number;
}

export interface QuickaddData {
  repos: Repo[];
  parents: ParentOption[];
}

export interface T3ProjectRow {
  path: string;
  name: string;
  is_git: boolean;
  added: boolean;
}

export interface NewLinkText {
  text: string;
  title: string | null;
}

export type Then = 'none' | 'do_it' | 'plan_it';

export interface CreateTaskInput {
  title: string;
  body: string | null;
  parent: TaskId | null;
  repos: RepoId[];
  links: NewLinkText[];
  then: Then;
}

export type DoItPlace = 'linked' | { repo: RepoId } | 'no_repo';

export type DoItResult =
  | { needs_choice: { choices: Repo[]; research_dir: string } }
  | { started: { report: LaunchReport; patch: Patch } };

export interface CreateTaskResult {
  id: TaskId;
  patch: Patch;
  launch: DoItResult | null;
  /** EXTRA: links that failed after the task existed ("Created 153, but couldn't link …"). */
  link_errors: { text: string; error: ServiceError | AppError }[];
}

export interface AddReposResult {
  repos: Repo[];
  patch: Patch;
}

export type CopyWhat = { task_number: TaskId } | 'agent_note' | 'setup_command';

export type OpenTarget = { url: string } | { link: LinkId };

export type FocusZone = 'list' | 'panel' | 'sidebar' | 'field' | 'overlay' | 'quickadd';

export interface FocusContext {
  window: 'main' | 'quickadd';
  zone: FocusZone;
  selection: TaskId | null;
  view: string;
}

export interface InstallCliResult {
  path: string;
  on_path: boolean;
}

// ---------- events (Appendix B) ----------

export interface NeedsChanged {
  version: number;
  items: NeedsItem[];
  count: number;
  checked_at: Timestamp | null;
}

export interface ConnectionsChanged {
  connections: Connection[];
  availability: Availability;
}

export interface MenuAction {
  /** A command id from `lib/logic/commands.ts`. */
  command: string;
}

export interface NavOpen {
  view: string | null;
  task: TaskId | null;
  needs_key: NeedsKey | null;
}

export interface QuickaddReset {
  hotkey: string;
}

export interface ToastEventAction {
  label: string;
  command: string;
  args: Record<string, unknown>;
}

export interface ToastEvent {
  text: string;
  actions: ToastEventAction[];
  undo: UndoEntry | null;
}

export interface AppearanceChanged {
  accent: string | null;
  theme: 'dark' | 'light';
}

export interface BackgroundError {
  source: string;
  message: string;
}
