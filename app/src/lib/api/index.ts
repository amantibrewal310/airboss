/*
  One typed function per command (SPEC-app Appendix A) and per event (Appendix B).
  Stores call these; components never do.
*/

import type {
  CopyWhat,
  CreateTaskInput,
  DoItPlace,
  FocusContext,
  LinkId,
  NeedsKey,
  OpenTarget,
  RepoId,
  Settings,
  Status,
  TaskId,
  ThreadKey,
  Timestamp,
} from '../types';
import type { ArgsOf, CommandName, EventName, Events, ResultOf } from './contract';
import { transport } from './transport';

function call<K extends CommandName>(command: K, args: ArgsOf<K>): Promise<ResultOf<K>> {
  return transport().invoke(command, args);
}

const none = {} as Record<string, never>;

export const api = {
  bootstrap: () => call('bootstrap', none),
  taskDetail: (id: TaskId, req: number, historyLimit: 30 | 200) => call('task_detail', { id, req, historyLimit }),
  runMap: (id: TaskId, req: number) => call('run_map', { id, req }),
  plans: (req: number) => call('plans', { req }),
  quickaddData: () => call('quickadd_data', none),
  t3Projects: () => call('t3_projects', none),
  createTask: (task: CreateTaskInput) => call('create_task', { task }),
  renameTask: (id: TaskId, title: string) => call('rename_task', { id, title }),
  editNotes: (id: TaskId, body: string | null) => call('edit_notes', { id, body }),
  setStatus: (id: TaskId, status: Status, note: string | null = null) => call('set_status', { id, status, note }),
  toggleDone: (id: TaskId) => call('toggle_done', { id }),
  acceptProposals: (ids: TaskId[]) => call('accept_proposals', { ids }),
  rejectProposals: (ids: TaskId[], reason: string | null = null) => call('reject_proposals', { ids, reason }),
  moveTask: (id: TaskId, parent: TaskId | null, position: number | null = null) =>
    call('move_task', { id, parent, position }),
  reorder: (id: TaskId, direction: 'up' | 'down') => call('reorder', { id, direction }),
  linkRepo: (id: TaskId, repo: RepoId) => call('link_repo', { id, repo }),
  unlinkRepo: (id: TaskId, repo: RepoId) => call('unlink_repo', { id, repo }),
  addRepos: (paths: string[]) => call('add_repos', { paths }),
  editRepo: (repo: RepoId, name: string, path: string, remote: string | null) => call('edit_repo', { repo, name, path, remote }),
  removeRepo: (repo: RepoId) => call('remove_repo', { repo }),
  backUpDatabase: () => call('back_up_database', none),
  pickFolder: () => call('pick_folder', none),
  setBranch: (id: TaskId, branch: string) => call('set_branch', { id, branch }),
  addDependency: (id: TaskId, waitsOn: TaskId) => call('add_dependency', { id, waitsOn }),
  removeDependency: (id: TaskId, waitsOn: TaskId) => call('remove_dependency', { id, waitsOn }),
  addLink: (id: TaskId, text: string, title: string | null = null) => call('add_link', { id, text, title }),
  removeLink: (link: LinkId) => call('remove_link', { link }),
  addNote: (id: TaskId, text: string) => call('add_note', { id, text }),
  doIt: (id: TaskId, place: DoItPlace | null = null, again = false) => call('do_it', { id, place, again }),
  planIt: (id: TaskId, again = false) => call('plan_it', { id, again }),
  addToT3: (repo: RepoId) => call('add_to_t3', { repo }),
  reopenAndStart: (id: TaskId) => call('reopen_and_start', { id }),
  openThread: (key: ThreadKey) => call('open_thread', { key }),
  openUrl: (target: OpenTarget) => call('open_url', { target }),
  needsDismiss: (key: NeedsKey) => call('needs_dismiss', { key }),
  needsSnooze: (key: NeedsKey, until: Timestamp | null = null) => call('needs_snooze', { key, until }),
  needsRestore: (key: NeedsKey) => call('needs_restore', { key }),
  undo: (entry: number | null = null) => call('undo', { entry }),
  recheckConnections: () => call('recheck_connections', none),
  copyText: (what: CopyWhat) => call('copy_text', { what }),
  installCli: () => call('install_cli', none),
  /** Connect Claude Code: `claude mcp add --scope user airboss …`, only on the user's click. */
  connectClaude: () => call('connect_claude', none),
  getSettings: () => call('get_settings', none),
  setSettings: (settings: Settings) => call('set_settings', { settings }),
  notificationPermission: () => call('notification_permission', none),
  openNotificationSettings: () => call('open_notification_settings', none),
  setFolded: (ids: TaskId[], folded: boolean) => call('set_folded', { ids, folded }),
  setFocusContext: (context: FocusContext) => call('set_focus_context', { context }),
  uiReady: (window: 'main' | 'quickadd') => call('ui_ready', { window }),
  quickaddShown: () => call('quickadd_shown', none),
  quickaddHide: () => call('quickadd_hide', none),
  quickaddResize: (height: number) => call('quickadd_resize', { height }),
  showMain: (view: string | null, task: TaskId | null = null, needsKey: NeedsKey | null = null) =>
    call('show_main', { view, task, needsKey }),
};

export type Api = typeof api;

/** Subscribe to a Rust event; resolves to an unsubscribe function. */
export function on<E extends EventName>(event: E, handler: (payload: Events[E]) => void): Promise<() => void> {
  return transport().listen(event, handler);
}

export type { Events, EventName, CommandName } from './contract';
