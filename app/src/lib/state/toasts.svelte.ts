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
  private timers = new Map<number, { left: number; started: number; timer: ReturnType<typeof setTimeout> | null }>();

  push(t: Omit<Toast, 'id'>): number {
    const id = this.next++;
    this.list = [...this.list, { ...t, id }].slice(-4);
    const ms = t.actions.length || t.undo ? ACTION_TOAST_MS : TOAST_MS;
    this.timers.set(id, { left: ms, started: Date.now(), timer: setTimeout(() => this.dismiss(id), ms) });
    return id;
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
    t.timer = setTimeout(() => this.dismiss(id), t.left);
  }
}
