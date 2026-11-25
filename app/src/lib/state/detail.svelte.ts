import type { TaskDetail, TaskId } from '../types';

/** Skeletons show only after this long, so fast loads don't flash. */
export const SKELETON_MS = 150;

/**
 * The selected task's detail. Every request carries a number; a response that isn't the
 * latest for the current selection is ignored.
 */
export class DetailStore {
  detail = $state.raw<TaskDetail | null>(null);
  skeleton = $state(false);
  error = $state<string | null>(null);
  limit = $state<30 | 200>(30);
  private req = 0;
  private wanted: TaskId | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** Starts a request; returns its number, or null if it should be dropped. */
  begin(id: TaskId): number {
    if (this.wanted !== id) this.limit = 30;
    this.wanted = id;
    const req = ++this.req;
    if (this.timer) clearTimeout(this.timer);
    if (this.detail?.task.id !== id) {
      this.timer = setTimeout(() => {
        if (this.req === req) this.skeleton = true;
      }, SKELETON_MS);
    }
    return req;
  }

  /** Accepts a response only if it answers the latest request for the current selection. */
  accept(d: TaskDetail): boolean {
    if (d.req !== this.req || d.task.id !== this.wanted) return false;
    if (this.timer) clearTimeout(this.timer);
    this.detail = d;
    this.skeleton = false;
    this.error = null;
    return true;
  }

  fail(req: number, message: string): void {
    if (req !== this.req) return;
    if (this.timer) clearTimeout(this.timer);
    this.skeleton = false;
    this.error = message;
  }

  clear(): void {
    this.wanted = null;
    this.req++;
    this.detail = null;
    this.skeleton = false;
    this.error = null;
  }

  get current(): TaskId | null {
    return this.wanted;
  }
}
