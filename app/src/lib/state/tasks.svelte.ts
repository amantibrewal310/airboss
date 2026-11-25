import type { Bootstrap, Patch, Repo, RowDto, TaskId } from '../types';
import { applyPatch } from '../logic/patch';

/**
 * Rows and order as `$state.raw`: no deep proxies over 2,000 objects. A patch swaps in a new
 * Map and replaces only the changed row objects.
 */
export class TasksStore {
  rows = $state.raw(new Map<TaskId, RowDto>());
  order = $state.raw<TaskId[]>([]);
  repos = $state.raw<Repo[]>([]);
  /** Ids to highlight as new (the 600 ms indigo wash). */
  fresh = $state.raw(new Set<TaskId>());
  /** Ids whose status just changed, with a token so the pop replays. */
  popped = $state.raw(new Map<TaskId, number>());
  /** Rows hidden locally while a reject waits out its undo window. */
  hidden = $state.raw(new Set<TaskId>());
  seq = 0;

  repoById = $derived(new Map(this.repos.map((r) => [r.id, r])));

  load(b: Pick<Bootstrap, 'rows' | 'order' | 'repos' | 'seq'>): void {
    this.rows = new Map(b.rows.map((r) => [r.id, r]));
    this.order = b.order;
    this.repos = b.repos;
    this.seq = b.seq;
  }

  /** Applies a patch once; returns false when it was stale. */
  apply(p: Patch): boolean {
    const before = this.rows;
    const next = applyPatch({ rows: this.rows, order: this.order, repos: this.repos, seq: this.seq }, p);
    if (!next) return false;
    this.rows = next.rows;
    if (next.order !== this.order) this.order = next.order;
    if (next.repos !== this.repos) this.repos = next.repos;
    this.seq = next.seq;
    const popped = new Map(this.popped);
    let pop = false;
    for (const r of p.upserts) {
      const old = before.get(r.id);
      if (old && (old.status !== r.status || old.waiting !== r.waiting)) {
        popped.set(r.id, (popped.get(r.id) ?? 0) + 1);
        pop = true;
      }
    }
    if (pop) this.popped = popped;
    if (p.created.length) {
      this.fresh = new Set([...this.fresh, ...p.created]);
      const created = p.created;
      setTimeout(() => {
        const left = new Set(this.fresh);
        for (const id of created) left.delete(id);
        this.fresh = left;
      }, 700);
    }
    return true;
  }

  hide(ids: TaskId[], on: boolean): void {
    const next = new Set(this.hidden);
    for (const id of ids) {
      if (on) next.add(id);
      else next.delete(id);
    }
    this.hidden = next;
  }

  get(id: TaskId | null): RowDto | undefined {
    return id === null ? undefined : this.rows.get(id);
  }
}
