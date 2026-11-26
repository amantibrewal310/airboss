import type { UndoEntry } from '../types';

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  text: string;
  /** A second line (a launch warning). */
  detail?: string;
  /** Orange warning icon for errors (red is for needs-me only). */
  tone: 'info' | 'warn';
  actions: ToastAction[];
  undo?: UndoEntry;
  /** An error whose fix is its action: it stays, with a close button, until used or closed. */
  sticky?: boolean;
}

/**
 * DESIGN: toasts leave after 2.8 s; with an action they stay 5 s and pause while hovered. An
 * error that carries an action (its fix, like "Add to T3 and retry") stays until it's used or
 * closed.
 */
export const TOAST_MS = 2800;
export const ACTION_TOAST_MS = 5000;
/** How many toasts show at once; the oldest goes first. */
export const MAX_TOASTS = 4;

/** Whether a toast stays until it's acted on or closed. */
export function isSticky(t: Pick<Toast, 'tone' | 'actions' | 'undo'>): boolean {
  return t.tone === 'warn' && t.actions.length > 0 && !t.undo;
}

interface Timer {
  left: number;
  started: number;
  timer: ReturnType<typeof setTimeout> | null;
  expired: boolean;
  sticky: boolean;
}

export class ToastStore {
  list = $state.raw<Toast[]>([]);
  private next = 1;
  private timers = new Map<number, Timer>();

  /**
   * `deadline` (ms since the epoch) is when the toast's actions stop being true (a deferred
   * reject is written): they go then, hovered or not.
   */
  push(t: Omit<Toast, 'id' | 'sticky'>, deadline?: number): number {
    const id = this.next++;
    const sticky = isSticky(t);
    const all = [...this.list, { ...t, id, ...(sticky ? { sticky } : {}) }];
    // Toasts pushed out of the stack stop their timers too (a sticky one has nothing else to end it).
    for (const old of all.slice(0, -MAX_TOASTS)) this.forget(old.id);
    this.list = all.slice(-MAX_TOASTS);
    const ms = t.actions.length || t.undo ? ACTION_TOAST_MS : TOAST_MS;
    this.timers.set(id, { left: ms, started: Date.now(), timer: sticky ? null : setTimeout(() => this.dismiss(id), ms), expired: false, sticky });
    if (deadline !== undefined) setTimeout(() => this.expire(id), Math.max(0, deadline - Date.now()));
    return id;
  }

  /** The toast's actions can't run any more: drop them, and let a hovered toast go when left. */
  private expire(id: number): void {
    const t = this.timers.get(id);
    if (!t) return;
    t.expired = true;
    this.list = this.list.map((x) => (x.id === id ? { ...x, actions: [], undo: undefined, sticky: false } : x));
    if (t.timer || t.sticky) {
      if (t.timer) clearTimeout(t.timer);
      t.sticky = false;
      t.timer = setTimeout(() => this.dismiss(id), 1200);
    }
  }

  private forget(id: number): void {
    const t = this.timers.get(id);
    if (t?.timer) clearTimeout(t.timer);
    this.timers.delete(id);
  }

  dismiss(id: number): void {
    this.forget(id);
    this.list = this.list.filter((x) => x.id !== id);
  }

  pause(id: number): void {
    const t = this.timers.get(id);
    if (!t || !t.timer) return;
    clearTimeout(t.timer);
    t.timer = null;
    t.left = Math.max(800, t.left - (Date.now() - t.started));
  }

  resume(id: number): void {
    const t = this.timers.get(id);
    if (!t || t.timer || t.sticky) return;
    t.started = Date.now();
    t.timer = setTimeout(() => this.dismiss(id), t.expired ? 1200 : t.left);
  }
}
