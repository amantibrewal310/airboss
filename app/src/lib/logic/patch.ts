import type { Patch, Repo, RowDto, TaskId } from '../types';

export interface RowsState {
  rows: Map<TaskId, RowDto>;
  order: TaskId[];
  repos: Repo[];
  seq: number;
}

/**
 * A write's patch arrives twice (returned and as an event); apply each seq once.
 * Returns the next state, or null when the patch is stale or empty. Only changed rows are
 * replaced, so every other row keeps its object identity and its component doesn't re-render.
 * Rust sends `seq: 0` for "nothing new": it is ignored and never read as going backwards.
 */
export function applyPatch(state: RowsState, p: Patch): RowsState | null {
  if (p.seq === 0) return null;
  if (p.seq <= state.seq) return null;
  const rows = new Map(state.rows);
  for (const r of p.upserts) rows.set(r.id, r);
  for (const id of p.removed) rows.delete(id);
  return {
    rows,
    order: p.order ?? (p.removed.length ? state.order.filter((id) => rows.has(id)) : state.order),
    repos: p.repos ?? state.repos,
    seq: p.seq,
  };
}
