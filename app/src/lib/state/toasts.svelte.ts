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
}

/** DESIGN: toasts leave after 2.8 s; with an action they stay 5 s and pause while hovered. */
export const TOAST_MS = 2800;
export const ACTION_TOAST_MS = 5000;

export class ToastStore {
  list = $state.raw<Toast[]>([]);
  private next = 1;
  private timers = new Map<number, { left: number; started: number; timer: ReturnType<typeof setTimeout> | null; expired: boolean }>();

  /**
   * `deadline` (ms since the epoch) is when the toast's actions stop being true (a deferred
   * reject is written): they go then, hovered or not.
   */
  push(t: Omit<Toast, 'id'>, deadline?: number): number {
    const id = this.next++;
    this.list = [...this.list, { ...t, id }].slice(-4);
    const ms = t.actions.length || t.undo ? ACTION_TOAST_MS : TOAST_MS;
    this.timers.set(id, { left: ms, started: Date.now(), timer: setTimeout(() => this.dismiss(id), ms), expired: false });
    if (deadline !== undefined) setTimeout(() => this.expire(id), Math.max(0, deadline - Date.now()));
    return id;
  }

  /** The toast's actions can't run any more: drop them, and let a hovered toast go when left. */
  private expire(id: number): void {
    const t = this.timers.get(id);
    if (!t) return;
    t.expired = true;
    this.list = this.list.map((x) => (x.id === id ? { ...x, actions: [], undo: undefined } : x));
    if (t.timer) {
      clearTimeout(t.timer);
      t.timer = setTimeout(() => this.dismiss(id), 1200);
    }
  }

  dismiss(id: number): void {
    const t = this.timers.get(id);
    if (t?.timer) clearTimeout(t.timer);
    this.timers.delete(id);
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
    if (!t || t.timer) return;
    t.started = Date.now();
    t.timer = setTimeout(() => this.dismiss(id), t.expired ? 1200 : t.left);
  }
}
