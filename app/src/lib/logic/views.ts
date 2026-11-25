/*
  Which rows each view shows, in what order and at what depth (SPEC-app "List views").
  Pure functions of (rows, order, view, folded, layout), wrapped in one $derived by the UI.
*/

import type { Repo, RowDto, Status, TaskId } from '../types';

export type View = 'needs' | 'ready' | 'plans' | 'progress' | 'all' | `repo:${number}` | `parent:${number}`;
export type Layout = 'tree' | 'status';
/** What a status icon draws: the status, or Waiting for a todo task with open blockers. */
export type Shape = Status | 'waiting';

export type VisibleItem =
  | {
      kind: 'row';
      key: string;
      id: TaskId;
      depth: number;
      /** 1-based position among visible siblings and their count (tree layouts, for ARIA). */
      pos: number;
      size: number;
    }
  | { kind: 'group'; key: string; shape: Shape; label: string; count: number };

export const SHAPE_LABEL: Record<Shape, string> = {
  proposed: 'Proposed',
  todo: 'Todo',
  doing: 'In progress',
  in_review: 'In review',
  blocked: 'Blocked',
  done: 'Done',
  dropped: 'Dropped',
  waiting: 'Waiting',
};

/** By status groups, in order. */
export const STATUS_GROUPS: Shape[] = ['doing', 'in_review', 'blocked', 'todo', 'waiting', 'proposed', 'done'];

export function shapeOf(row: Pick<RowDto, 'status' | 'waiting'>): Shape {
  return row.waiting && row.status === 'todo' ? 'waiting' : row.status;
}

export const isOpenStatus = (s: Status) => s === 'todo' || s === 'doing' || s === 'in_review' || s === 'blocked';

export function parseView(view: View): { kind: 'needs' | 'ready' | 'plans' | 'progress' | 'all' | 'repo' | 'parent'; id: number | null } {
  if (view.startsWith('repo:')) return { kind: 'repo', id: Number(view.slice(5)) };
  if (view.startsWith('parent:')) return { kind: 'parent', id: Number(view.slice(7)) };
  return { kind: view as 'needs', id: null };
}

export function isView(text: string): text is View {
  return /^(needs|ready|plans|progress|all|repo:\d+|parent:\d+)$/.test(text);
}

/** Views drawn as the task list (everything but Needs me and Plans). */
export function isListView(view: View): boolean {
  return view !== 'needs' && view !== 'plans';
}

/** Tree layouts allow folding and reordering: All tasks (Tree) and a parent's view. */
export function isTreeLayout(view: View, layout: Layout): boolean {
  const k = parseView(view).kind;
  return (k === 'all' && layout === 'tree') || k === 'parent';
}

function flat(rows: Map<TaskId, RowDto>, order: TaskId[], keep: (r: RowDto) => boolean): VisibleItem[] {
  const out: VisibleItem[] = [];
  for (const id of order) {
    const r = rows.get(id);
    if (r && keep(r)) out.push({ kind: 'row', key: `r${id}`, id, depth: 0, pos: 0, size: 0 });
  }
  return out;
}

/** Walks `order` from `start`, keeping rows deeper than `floor`, skipping folded and dropped subtrees. */
function tree(rows: Map<TaskId, RowDto>, order: TaskId[], start: number, floor: number, folded: Set<TaskId>): VisibleItem[] {
  const out: (VisibleItem & { kind: 'row' })[] = [];
  let skipDeeperThan = Infinity;
  for (let i = start; i < order.length; i++) {
    const r = rows.get(order[i]!);
    if (!r) continue;
    if (r.depth <= floor) break;
    if (r.depth > skipDeeperThan) continue;
    skipDeeperThan = Infinity;
    if (r.status === 'dropped') {
      skipDeeperThan = r.depth;
      continue;
    }
    out.push({ kind: 'row', key: `r${r.id}`, id: r.id, depth: r.depth - floor - 1, pos: 0, size: 0 });
    if (r.has_children && folded.has(r.id)) skipDeeperThan = r.depth;
  }
  // ARIA set sizes and positions among visible siblings.
  const byParent = new Map<TaskId | null, (VisibleItem & { kind: 'row' })[]>();
  for (const item of out) {
    const p = rows.get(item.id)!.parent;
    const list = byParent.get(p) ?? [];
    list.push(item);
    byParent.set(p, list);
  }
  for (const list of byParent.values()) list.forEach((item, i) => ((item.pos = i + 1), (item.size = list.length)));
  return out;
}

export function visibleItems(
  rows: Map<TaskId, RowDto>,
  order: TaskId[],
  view: View,
  folded: Set<TaskId>,
  layout: Layout,
): VisibleItem[] {
  const { kind, id } = parseView(view);
  switch (kind) {
    case 'needs':
    case 'plans':
      return [];
    case 'ready':
      return flat(rows, order, (r) => r.ready);
    case 'progress':
      return flat(rows, order, (r) => r.status === 'doing' || r.status === 'in_review');
    case 'repo':
      return flat(rows, order, (r) => r.status !== 'dropped' && r.repos.includes(id!));
    case 'parent': {
      const at = order.indexOf(id!);
      const p = rows.get(id!);
      return at === -1 || !p ? [] : tree(rows, order, at + 1, p.depth, folded);
    }
    case 'all': {
      if (layout === 'tree') return tree(rows, order, 0, -1, folded);
      const items: VisibleItem[] = [];
      for (const shape of STATUS_GROUPS) {
        const group = flat(rows, order, (r) => r.status !== 'dropped' && shapeOf(r) === shape);
        if (!group.length) continue;
        items.push({ kind: 'group', key: `g${shape}`, shape, label: SHAPE_LABEL[shape], count: group.length });
        items.push(...group);
      }
      return items;
    }
  }
}

/** The header's count: open tasks the view holds, whatever is folded. */
export function viewCount(rows: Map<TaskId, RowDto>, order: TaskId[], view: View): number {
  const items = visibleItems(rows, order, view, new Set(), 'tree');
  return items.filter((i) => i.kind === 'row' && isOpenStatus(rows.get(i.id)!.status)).length;
}

export function viewTitle(view: View, rows: Map<TaskId, RowDto>, repos: Repo[]): string {
  const { kind, id } = parseView(view);
  switch (kind) {
    case 'needs':
      return 'Needs me';
    case 'ready':
      return 'Ready to start';
    case 'plans':
      return 'Plans';
    case 'progress':
      return 'In progress';
    case 'all':
      return 'All tasks';
    case 'repo':
      return repos.find((r) => r.id === id)?.name ?? 'Repo';
    case 'parent':
      return rows.get(id!)?.title ?? 'Task';
  }
}

export interface SidebarCounts {
  ready: number;
  plans: number;
  progress: number;
  all: number;
}

/** Tasks with at least one accepted subtask (proposals alone don't make a plan yet). */
export function plannedParents(rows: Map<TaskId, RowDto>): Set<TaskId> {
  const out = new Set<TaskId>();
  for (const r of rows.values()) {
    if (r.parent !== null && r.status !== 'proposed' && r.status !== 'dropped') out.add(r.parent);
  }
  return out;
}

export function sidebarCounts(rows: Map<TaskId, RowDto>): SidebarCounts {
  const c: SidebarCounts = { ready: 0, plans: 0, progress: 0, all: 0 };
  const planned = plannedParents(rows);
  for (const r of rows.values()) {
    if (r.ready) c.ready++;
    if (r.status === 'doing' || r.status === 'in_review') c.progress++;
    if (isOpenStatus(r.status)) c.all++;
    if (r.parent === null && planned.has(r.id) && isOpenStatus(r.status)) c.plans++;
  }
  return c;
}

/** Sidebar › Parents: open top-level tasks with (accepted) subtasks, in position order. */
export function parentsList(rows: Map<TaskId, RowDto>, order: TaskId[]): RowDto[] {
  const planned = plannedParents(rows);
  const out: RowDto[] = [];
  for (const id of order) {
    const r = rows.get(id)!;
    if (r.parent === null && planned.has(r.id) && isOpenStatus(r.status)) out.push(r);
  }
  return out;
}

export function percent(p: { done: number; total: number } | null): number {
  return p && p.total ? Math.round((p.done / p.total) * 100) : 0;
}

/** Ancestors of a task, nearest first. */
export function ancestorsOf(rows: Map<TaskId, RowDto>, id: TaskId): TaskId[] {
  const out: TaskId[] = [];
  let p = rows.get(id)?.parent ?? null;
  while (p !== null) {
    out.push(p);
    p = rows.get(p)?.parent ?? null;
  }
  return out;
}

/** Whether a view would show this task (folds aside). */
export function viewHolds(rows: Map<TaskId, RowDto>, order: TaskId[], view: View, id: TaskId): boolean {
  return visibleItems(rows, order, view, new Set(), 'tree').some((i) => i.kind === 'row' && i.id === id);
}
