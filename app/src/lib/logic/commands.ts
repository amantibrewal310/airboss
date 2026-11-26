/*
  The one command registry (SPEC-app "Keyboard, focus and menus"). The keymap, the native Task
  and View menus (by id, through `menu:action`), the context menu, ⌘K's command list and the
  shortcut sheet are all built from it.
*/

import type { KeyGroup, Zone } from './focus';
import { allowed } from './focus';

export type ViewKind = 'list' | 'needs' | 'plans' | 'page';

/** What commands act on: implemented by the main window's controller (and by fakes in tests). */
export interface CommandHost {
  readonly viewKind: ViewKind;
  readonly zone: Zone;
  readonly selection: number | null;
  readonly hasNeeds: boolean;
  run(id: CommandId): void;
}

export interface Command {
  id: CommandId;
  label: string;
  /** Each key with the group whose focus rule it follows. */
  bind: [string, KeyGroup][];
  /** Views where the keys act (default: all). Menus and ⌘K can run a command anywhere it's enabled. */
  views?: ViewKind[];
  /** A selected task is needed. */
  needsTask?: boolean;
  /** The native menu it belongs to. */
  menu?: 'task' | 'view' | 'file' | 'app';
  /** Offered in ⌘K's command list. */
  palette?: boolean;
}

/** Every command id; native menu items send these too (app/src-tauri/src/menu.rs). */
export const COMMAND_IDS = [
  'go_needs',
  'go_plans',
  'go_ready',
  'go_all',
  'go_progress',
  'open_needs',
  'jump',
  'new_task',
  'quick_add',
  'settings',
  'shortcuts',
  'undo',
  'toasts',
  'up',
  'down',
  'first',
  'last',
  'page_up',
  'page_down',
  'open_fold',
  'close_fold',
  'rename',
  'toggle_done',
  'status',
  'link_repos',
  'do_it',
  'plan_it',
  'open_page',
  'back',
  'move_up',
  'move_down',
  'add_waits_on',
  'add_link',
  'mark_blocked',
  'open_latest',
  'drop',
  'copy_number',
  'escape',
  'to_panel',
  'needs_up',
  'needs_down',
  'needs_main',
  'needs_dismiss',
  'needs_done',
  'plans_left',
  'plans_right',
  'plans_up',
  'plans_down',
  'plans_open',
  'layout_tree',
  'layout_status',
  'move_under',
  'check_connections',
  'check_for_updates',
] as const;

export type CommandId = (typeof COMMAND_IDS)[number];

const LIST: ViewKind[] = ['list'];
const TASKS: ViewKind[] = ['list', 'page'];

export const COMMANDS: Command[] = [
  // Views
  { id: 'go_needs', label: 'Go to Needs me', bind: [['g n', 'letter']], menu: 'view', palette: true },
  { id: 'go_plans', label: 'Go to Plans', bind: [['g l', 'letter']], menu: 'view', palette: true },
  { id: 'go_ready', label: 'Go to Ready to start', bind: [['g r', 'letter']], menu: 'view', palette: true },
  { id: 'go_all', label: 'Go to All tasks', bind: [['g a', 'letter']], menu: 'view', palette: true },
  { id: 'go_progress', label: 'Go to In progress', bind: [['g p', 'letter']], menu: 'view', palette: true },
  { id: 'open_needs', label: 'Open Airboss on Needs me', bind: [['mod+0', 'window']], menu: 'view' },
  { id: 'layout_tree', label: 'Show as tree', bind: [], menu: 'view', palette: true },
  { id: 'layout_status', label: 'Show by status', bind: [], menu: 'view', palette: true },
  // App
  { id: 'jump', label: 'Jump to a task', bind: [['mod+k', 'global']], menu: 'view' },
  { id: 'new_task', label: 'New task', bind: [['mod+n', 'global'], ['c', 'letter'], ['alt+n', 'letter']], menu: 'file', palette: true },
  { id: 'settings', label: 'Settings…', bind: [['mod+comma', 'window']], menu: 'app', palette: true },
  { id: 'shortcuts', label: 'Keyboard shortcuts', bind: [['?', 'letter']], menu: 'app', palette: true },
  { id: 'undo', label: 'Undo', bind: [['mod+z', 'undo']], menu: 'task' },
  { id: 'toasts', label: 'Go to notifications', bind: [['f6', 'f6']] },
  { id: 'check_connections', label: 'Check connections again', bind: [], palette: true },
  // ⌘K, and Airboss › Check for Updates…, which Rust forwards as `menu:action`.
  { id: 'check_for_updates', label: 'Check for updates', bind: [], menu: 'app', palette: true },
  // The list
  { id: 'up', label: 'Move up', bind: [['up', 'nav']], views: LIST },
  { id: 'down', label: 'Move down', bind: [['down', 'nav']], views: LIST },
  { id: 'first', label: 'First row', bind: [['home', 'nav'], ['alt+up', 'nav']], views: LIST },
  { id: 'last', label: 'Last row', bind: [['end', 'nav'], ['alt+down', 'nav']], views: LIST },
  { id: 'page_up', label: 'Page up', bind: [['pageup', 'nav']], views: LIST },
  { id: 'page_down', label: 'Page down', bind: [['pagedown', 'nav']], views: LIST },
  { id: 'open_fold', label: 'Open, or go to the first subtask', bind: [['right', 'nav']], views: LIST, needsTask: true },
  { id: 'close_fold', label: 'Fold, or go to the parent', bind: [['left', 'nav']], views: LIST, needsTask: true },
  { id: 'rename', label: 'Rename', bind: [['enter', 'nav']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'to_panel', label: 'Switch list / detail', bind: [['tab', 'nav']], views: LIST, needsTask: true },
  // The selected task
  { id: 'toggle_done', label: 'Mark done', bind: [['space', 'letter']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'status', label: 'Status…', bind: [['s', 'letter']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'link_repos', label: 'Link repos…', bind: [['r', 'letter']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'do_it', label: 'Do it', bind: [['mod+enter', 'task']], views: TASKS, needsTask: true, menu: 'task', palette: true },
  { id: 'plan_it', label: 'Plan it', bind: [['shift+mod+enter', 'task']], views: TASKS, needsTask: true, menu: 'task', palette: true },
  { id: 'open_page', label: 'Open page', bind: [['mod+down', 'task']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'back', label: 'Back to the list', bind: [['mod+up', 'task']], views: ['page'], menu: 'view' },
  { id: 'move_up', label: 'Move up', bind: [['alt+mod+up', 'task']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'move_down', label: 'Move down', bind: [['alt+mod+down', 'task']], views: LIST, needsTask: true, menu: 'task' },
  { id: 'add_waits_on', label: 'Add waits-on…', bind: [['mod+d', 'task']], views: TASKS, needsTask: true, menu: 'task', palette: true },
  { id: 'move_under', label: 'Move under…', bind: [], needsTask: true, menu: 'task', palette: true },
  { id: 'add_link', label: 'Add link…', bind: [['mod+l', 'task']], views: TASKS, needsTask: true, menu: 'task', palette: true },
  { id: 'mark_blocked', label: 'Mark blocked…', bind: [['mod+b', 'task']], views: TASKS, needsTask: true, menu: 'task' },
  { id: 'open_latest', label: 'Open latest thread or PR', bind: [['mod+o', 'task']], views: TASKS, needsTask: true, menu: 'task', palette: true },
  { id: 'drop', label: 'Drop', bind: [['mod+backspace', 'task']], views: TASKS, needsTask: true, menu: 'task' },
  { id: 'copy_number', label: 'Copy task number', bind: [['shift+mod+c', 'task']], views: TASKS, needsTask: true, menu: 'task', palette: true },
  { id: 'escape', label: 'Close', bind: [['esc', 'esc']] },
  // Needs me
  { id: 'needs_up', label: 'Previous item', bind: [['up', 'nav']], views: ['needs'] },
  { id: 'needs_down', label: 'Next item', bind: [['down', 'nav']], views: ['needs'] },
  { id: 'needs_main', label: 'Main action', bind: [['enter', 'nav'], ['mod+enter', 'task']], views: ['needs'] },
  { id: 'needs_dismiss', label: 'Dismiss', bind: [['mod+backspace', 'task']], views: ['needs'] },
  { id: 'needs_done', label: 'Mark done', bind: [['space', 'letter']], views: ['needs'] },
  // Plans
  { id: 'plans_left', label: 'Previous plan', bind: [['left', 'nav']], views: ['plans'] },
  { id: 'plans_right', label: 'Next plan', bind: [['right', 'nav']], views: ['plans'] },
  { id: 'plans_up', label: 'Plan above', bind: [['up', 'nav']], views: ['plans'] },
  { id: 'plans_down', label: 'Plan below', bind: [['down', 'nav']], views: ['plans'] },
  { id: 'plans_open', label: 'Open plan', bind: [['enter', 'nav']], views: ['plans'] },
  // The quick-add hotkey is global (Rust); listed here for the sheet and menus.
  { id: 'quick_add', label: 'Quick add from any app', bind: [], menu: 'file' },
];

export const BY_ID = new Map(COMMANDS.map((c) => [c.id, c]));

/** The command a key press runs, given the focus zone and the view, or null. */
export function resolve(key: string, zone: Zone, view: ViewKind, hasSelection: boolean): Command | null {
  for (const c of COMMANDS) {
    for (const [k, group] of c.bind) {
      if (k !== key || !allowed(group, zone)) continue;
      if (c.views && !c.views.includes(view)) continue;
      if (c.needsTask && !hasSelection) continue;
      return c;
    }
  }
  return null;
}

/** The keys shown for a command (its first binding). */
export function keysOf(id: CommandId): string {
  return BY_ID.get(id)?.bind[0]?.[0] ?? '';
}

/**
 * The shortcut sheet, in DESIGN's two-column order (row by row), plus ⌥⌘↑↓, ⌘Z and F6.
 * `hotkey` is the quick-add key from Settings.
 */
export function sheetRows(hotkey: string): { label: string; keys: string }[] {
  return [
    { label: 'Needs me', keys: 'g n' },
    { label: 'Ready to start', keys: 'g r' },
    { label: 'All tasks', keys: 'g a' },
    { label: 'Jump to task', keys: 'mod+k' },
    { label: 'New task', keys: 'c' },
    { label: 'Quick add from any app', keys: hotkey },
    { label: 'Do it', keys: 'mod+enter' },
    { label: 'Plan it', keys: 'shift+mod+enter' },
    { label: 'Move', keys: 'up+down' },
    { label: 'Open or fold', keys: 'right+left' },
    { label: 'Mark done', keys: 'space' },
    { label: 'Change status', keys: 's' },
    { label: 'Plans', keys: 'g l' },
    { label: 'Rename', keys: 'enter' },
    { label: 'Open full page', keys: 'mod+down' },
    { label: 'Close the page or panel', keys: 'esc' },
    { label: 'Open thread or PR', keys: 'mod+o' },
    { label: 'Add waits-on', keys: 'mod+d' },
    { label: 'Add link', keys: 'mod+l' },
    { label: 'Mark blocked', keys: 'mod+b' },
    { label: 'Needs me: main action', keys: 'enter' },
    { label: 'Needs me: dismiss', keys: 'mod+backspace' },
    { label: 'Switch between list and detail', keys: 'tab' },
    { label: 'This sheet', keys: '?' },
    { label: 'Reorder', keys: 'alt+mod+up+down' },
    { label: 'Undo', keys: 'mod+z' },
    { label: 'In progress', keys: 'g p' },
    { label: 'Notifications', keys: 'f6' },
  ];
}

/** The right-click menu, in DESIGN's order. */
export const CONTEXT_MENU: (CommandId | '-')[] = [
  'do_it',
  'plan_it',
  '-',
  'status',
  'toggle_done',
  'rename',
  'link_repos',
  'add_waits_on',
  'move_up',
  'move_down',
  'open_page',
  'copy_number',
  '-',
  'drop',
];
