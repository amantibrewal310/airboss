/*
  The command and event contract with Rust: every command in SPEC-app.md Appendix A and every
  event in Appendix B, with its argument and result types. Argument keys are the JS names Tauri
  expects (it maps Rust's snake_case parameters to camelCase); struct fields inside stay
  snake_case, as serde writes them.
*/

import type {
  AddReposResult,
  AppError,
  AppearanceChanged,
  BackgroundError,
  Bootstrap,
  ConnectionsChanged,
  CopyWhat,
  CreateTaskInput,
  CreateTaskResult,
  DoItPlace,
  DoItResult,
  FocusContext,
  InstallCliResult,
  LinkId,
  MenuAction,
  NavOpen,
  NeedsChanged,
  NeedsKey,
  NotificationPermission,
  OpenTarget,
  Patch,
  PlanCard,
  Priority,
  UpdateStatus,
  QuickaddData,
  QuickaddReset,
  RemoveRepoResult,
  RepoId,
  RunMap,
  Settings,
  Status,
  T3ProjectRow,
  TaskDetail,
  TaskId,
  ThreadKey,
  Timestamp,
  ToastEvent,
  UndoableWrite,
  UndoResult,
} from '../types';

type C<A, R> = { args: A; result: R };
type None = Record<string, never>;

export interface Commands {
  bootstrap: C<None, Bootstrap>;
  task_detail: C<{ id: TaskId; req: number; historyLimit: number }, TaskDetail>;
  run_map: C<{ id: TaskId; req: number }, RunMap>;
  plans: C<{ req: number }, PlanCard[]>;
  quickadd_data: C<None, QuickaddData>;
  t3_projects: C<None, T3ProjectRow[]>;
  herdr_folders: C<None, T3ProjectRow[]>;
  create_task: C<{ task: CreateTaskInput }, CreateTaskResult>;
  rename_task: C<{ id: TaskId; title: string }, Patch>;
  edit_notes: C<{ id: TaskId; body: string | null }, Patch>;
  set_status: C<{ id: TaskId; status: Status; note: string | null }, UndoableWrite>;
  toggle_done: C<{ id: TaskId }, UndoableWrite>;
  accept_proposals: C<{ ids: TaskId[] }, Patch>;
  reject_proposals: C<{ ids: TaskId[]; reason: string | null }, UndoableWrite>;
  move_task: C<{ id: TaskId; parent: TaskId | null; position: number | null }, UndoableWrite>;
  reorder: C<{ id: TaskId; direction: 'up' | 'down' }, UndoableWrite>;
  link_repo: C<{ id: TaskId; repo: RepoId }, Patch>;
  unlink_repo: C<{ id: TaskId; repo: RepoId }, Patch>;
  add_repos: C<{ paths: string[] }, AddReposResult>;
  edit_repo: C<{ repo: RepoId; name: string; path: string; remote: string | null }, Patch>;
  remove_repo: C<{ repo: RepoId }, RemoveRepoResult>;
  back_up_database: C<None, string | null>;
  pick_folder: C<None, string | null>;
  set_branch: C<{ id: TaskId; branch: string }, Patch>;
  set_due: C<{ id: TaskId; due: string }, Patch>;
  set_priority: C<{ id: TaskId; priority: Priority | null }, Patch>;
  add_dependency: C<{ id: TaskId; waitsOn: TaskId }, Patch>;
  remove_dependency: C<{ id: TaskId; waitsOn: TaskId }, Patch>;
  add_link: C<{ id: TaskId; text: string; title: string | null }, Patch>;
  remove_link: C<{ link: LinkId }, Patch>;
  add_note: C<{ id: TaskId; text: string }, Patch>;
  do_it: C<{ id: TaskId; place: DoItPlace | null; again: boolean }, DoItResult>;
  plan_it: C<{ id: TaskId; again: boolean }, DoItResult>;
  add_to_t3: C<{ repo: RepoId }, null>;
  reopen_and_start: C<{ id: TaskId }, DoItResult>;
  open_thread: C<{ key: ThreadKey }, null>;
  open_url: C<{ target: OpenTarget }, null>;
  needs_dismiss: C<{ key: NeedsKey }, UndoableWrite>;
  needs_snooze: C<{ key: NeedsKey; until: Timestamp | null }, UndoableWrite>;
  needs_restore: C<{ key: NeedsKey }, null>;
  undo: C<{ entry: number | null }, UndoResult>;
  recheck_connections: C<None, null>;
  copy_text: C<{ what: CopyWhat }, null>;
  install_cli: C<None, InstallCliResult>;
  connect_claude: C<None, null>;
  get_settings: C<None, Settings>;
  set_settings: C<{ settings: Settings }, Settings>;
  notification_permission: C<None, NotificationPermission>;
  open_notification_settings: C<None, null>;
  set_folded: C<{ ids: TaskId[]; folded: boolean }, null>;
  set_focus_context: C<{ context: FocusContext }, null>;
  ui_ready: C<{ window: 'main' | 'quickadd' }, null>;
  quickadd_shown: C<None, null>;
  quickadd_hide: C<None, null>;
  quickadd_resize: C<{ height: number }, null>;
  show_main: C<{ view: string | null; task: TaskId | null; needsKey: NeedsKey | null }, null>;
  /** In-app updates (SPEC-updates). */
  update_status: C<None, UpdateStatus>;
  check_for_updates: C<None, UpdateStatus>;
  install_update: C<None, null>;
}

export interface Events {
  'tree:patch': Patch;
  'needs:changed': NeedsChanged;
  'connections:changed': ConnectionsChanged;
  'menu:action': MenuAction;
  'nav:open': NavOpen;
  'quickadd:reset': QuickaddReset;
  toast: ToastEvent;
  'appearance:changed': AppearanceChanged;
  'error:background': BackgroundError;
  /** The database broke while the app ran (R2): the full-window database problem state. */
  'db:unavailable': AppError;
  /** The in-app update's status changed (SPEC-updates). */
  'update:changed': UpdateStatus;
}

export type CommandName = keyof Commands;
export type EventName = keyof Events;
export type ArgsOf<K extends CommandName> = Commands[K]['args'];
export type ResultOf<K extends CommandName> = Commands[K]['result'];

/** Every command name, in Appendix A order (used by tests and the mock's coverage check). */
export const COMMAND_NAMES = [
  'bootstrap', 'task_detail', 'run_map', 'plans', 'quickadd_data', 't3_projects', 'herdr_folders', 'create_task',
  'rename_task', 'edit_notes', 'set_status', 'toggle_done', 'accept_proposals', 'reject_proposals',
  'move_task', 'reorder', 'link_repo', 'unlink_repo', 'add_repos', 'edit_repo', 'remove_repo',
  'back_up_database', 'pick_folder', 'set_branch', 'set_due', 'set_priority',
  'add_dependency', 'remove_dependency', 'add_link', 'remove_link', 'add_note', 'do_it', 'plan_it',
  'add_to_t3', 'reopen_and_start', 'open_thread', 'open_url', 'needs_dismiss', 'needs_snooze',
  'needs_restore', 'undo', 'recheck_connections', 'copy_text', 'install_cli', 'connect_claude', 'get_settings',
  'set_settings', 'notification_permission', 'open_notification_settings', 'set_folded', 'set_focus_context', 'ui_ready', 'quickadd_shown', 'quickadd_hide',
  'quickadd_resize', 'show_main', 'update_status', 'check_for_updates', 'install_update',
] as const satisfies readonly CommandName[];

export const EVENT_NAMES = [
  'tree:patch', 'needs:changed', 'connections:changed', 'menu:action', 'nav:open',
  'quickadd:reset', 'toast', 'appearance:changed', 'error:background', 'db:unavailable',
  'update:changed',
] as const satisfies readonly EventName[];
