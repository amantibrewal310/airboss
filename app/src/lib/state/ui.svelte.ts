import type { DoItPlace, FocusZone, Repo, RepoId, TaskId } from '../types';
import type { Layout, View } from '../logic/views';

export interface Anchor {
  x: number;
  y: number;
  /** Width of the thing the popover hangs from (to align a right edge). */
  w?: number;
}

/** Every overlay the main window can show. One at a time; Esc closes it. */
export type Overlay =
  | { kind: 'jump'; mode: 'jump' }
  | { kind: 'jump'; mode: 'waits_on' | 'move_under'; task: TaskId }
  | { kind: 'new'; parent: TaskId | null; repos: RepoId[]; plan: boolean }
  | { kind: 'sheet' }
  | { kind: 'settings' }
  | { kind: 'status'; task: TaskId; at: Anchor }
  | { kind: 'context'; task: TaskId; at: Anchor }
  | { kind: 'repos'; task: TaskId; at: Anchor }
  | { kind: 'start_in'; task: TaskId; at: Anchor; choices: Repo[]; researchDir: string; then?: (place: DoItPlace) => void }
  | { kind: 'blocked'; task: TaskId; at: Anchor }
  | { kind: 'drop'; task: TaskId; open: number }
  | { kind: 'link'; task: TaskId; at: Anchor }
  | { kind: 'add_repos' }
  | { kind: 'connections' };

export class UiStore {
  view = $state<View>('needs');
  layout = $state<Layout>('tree');
  selected = $state<TaskId | null>(null);
  panelOpen = $state(false);
  pageOpen = $state(false);
  zone = $state<FocusZone>('list');
  overlay = $state.raw<Overlay | null>(null);
  renaming = $state<TaskId | null>(null);
  /** Tasks whose Do it is in flight ("Starting…"). */
  starting = $state.raw(new Set<TaskId>());
  mapSel = $state<TaskId | null>(null);
  folded = $state.raw(new Set<TaskId>());
  planIndex = $state(0);
  /** The last selection came from the keyboard: draw the inset outline. */
  keyboard = $state(true);
  /** Index of the selection in the visible list, for the "same index" rule. */
  selectedIndex = -1;
  /** Where focus goes back when an overlay closes. */
  returnFocus: HTMLElement | null = null;

  setStarting(id: TaskId, on: boolean): void {
    const next = new Set(this.starting);
    if (on) next.add(id);
    else next.delete(id);
    this.starting = next;
  }
}
