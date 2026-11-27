/*
  The mock's view model: builds what Rust's `viewmodel.rs` and `runmap.rs` will build, from
  the mock model. Pure functions of the state, so tests can check each rule.
*/

import type {
  Chip,
  Source,
  DepDto,
  HistoryEntry,
  LinkDto,
  NeedsItem,
  ParentOption,
  Patch,
  PlanCard,
  Prepared,
  Progress,
  Repo,
  RepoId,
  RowDto,
  RunEdge,
  RunMap,
  Task,
  TaskDetail,
  TaskId,
  ThreadDto,
  Timestamp,
} from '../types';
import { isOpen, threadId, type MockState, type MockThread } from './model';

const DAY = 86_400_000;

export interface TreeIndex {
  children: Map<TaskId | null, TaskId[]>;
  order: TaskId[];
  depth: Map<TaskId, number>;
  blockers: Map<TaskId, TaskId[]>;
  dependents: Map<TaskId, TaskId[]>;
}

export function indexTree(s: MockState): TreeIndex {
  const children = new Map<TaskId | null, TaskId[]>();
  for (const t of s.tasks.values()) {
    const list = children.get(t.parent) ?? [];
    list.push(t.id);
    children.set(t.parent, list);
  }
  for (const list of children.values()) {
    list.sort((a, b) => s.tasks.get(a)!.position - s.tasks.get(b)!.position || a - b);
  }
  const order: TaskId[] = [];
  const depth = new Map<TaskId, number>();
  const walk = (parent: TaskId | null, d: number) => {
    for (const id of children.get(parent) ?? []) {
      order.push(id);
      depth.set(id, d);
      walk(id, d + 1);
    }
  };
  walk(null, 0);
  const blockers = new Map<TaskId, TaskId[]>();
  const dependents = new Map<TaskId, TaskId[]>();
  for (const [task, on] of s.edges) {
    if (!s.tasks.has(task) || !s.tasks.has(on)) continue;
    blockers.set(task, [...(blockers.get(task) ?? []), on]);
    dependents.set(on, [...(dependents.get(on) ?? []), task]);
  }
  return { children, order, depth, blockers, dependents };
}

const kids = (ix: TreeIndex, id: TaskId | null) => ix.children.get(id) ?? [];

export function liveKids(s: MockState, ix: TreeIndex, id: TaskId): TaskId[] {
  return kids(ix, id).filter((k) => s.tasks.get(k)!.status !== 'dropped');
}

/** Proposals I rejected: dropped, with a reject as their latest status change. */
function rejectedIds(s: MockState): Set<TaskId> {
  const last = new Map<TaskId, string>();
  for (const e of s.events) if (e.kind === 'status' || e.kind === 'rejected') last.set(e.task, e.kind);
  return new Set([...s.tasks.values()].filter((t) => t.status === 'dropped' && last.get(t.id) === 'rejected').map((t) => t.id));
}

/** A task's steps: subtasks that aren't proposals, open or rejected (SPEC-core rule 10). */
export function stepsOf(s: MockState, ix: TreeIndex, id: TaskId, rejected: Set<TaskId> = rejectedIds(s)): TaskId[] {
  return kids(ix, id).filter((k) => s.tasks.get(k)!.status !== 'proposed' && !rejected.has(k));
}

/**
 * SPEC-core rule 10, which Rust works out on every read: a task with steps shows what they add
 * up to. The mock writes it into the task, deepest first, before each rebuild. Dropped when
 * every step is; done when every step is done or dropped; blocked when any step is; in
 * progress when any is doing or in review, or some are finished and others not; else todo.
 */
export function rollUp(s: MockState): void {
  const ix = indexTree(s);
  const rejected = rejectedIds(s);
  const parents = new Set<TaskId>();
  for (const id of [...ix.order].reverse()) {
    const steps = stepsOf(s, ix, id, rejected).map((k) => s.tasks.get(k)!);
    if (!steps.length) continue;
    parents.add(id);
    const t = s.tasks.get(id)!;
    const finished = steps.filter((x) => x.status === 'done' || x.status === 'dropped').length;
    const blocked = steps.find((x) => x.status === 'blocked');
    t.blocked_reason = null;
    if (steps.every((x) => x.status === 'dropped')) t.status = 'dropped';
    else if (finished === steps.length) t.status = 'done';
    else if (blocked) {
      t.status = 'blocked';
      t.blocked_reason = parents.has(blocked.id) ? blocked.blocked_reason : `#${blocked.id} is blocked: ${blocked.blocked_reason ?? ''}`;
    } else if (finished > 0 || steps.some((x) => x.status === 'doing' || x.status === 'in_review')) t.status = 'doing';
    else t.status = 'todo';
  }
}

export function subtree(ix: TreeIndex, id: TaskId): TaskId[] {
  const out: TaskId[] = [];
  const walk = (p: TaskId) => {
    for (const k of kids(ix, p)) {
      out.push(k);
      walk(k);
    }
  };
  walk(id);
  return out;
}

export function ancestors(s: MockState, id: TaskId): TaskId[] {
  const out: TaskId[] = [];
  let p = s.tasks.get(id)?.parent ?? null;
  while (p !== null) {
    out.push(p);
    p = s.tasks.get(p)?.parent ?? null;
  }
  return out;
}

export function openBlockers(s: MockState, ix: TreeIndex, id: TaskId): TaskId[] {
  return (ix.blockers.get(id) ?? []).filter((b) => isOpen(s.tasks.get(b)!.status)).sort((a, b) => a - b);
}

export function isWaiting(s: MockState, ix: TreeIndex, id: TaskId): boolean {
  return s.tasks.get(id)!.status === 'todo' && openBlockers(s, ix, id).length > 0;
}

export function progressOf(s: MockState, ix: TreeIndex, id: TaskId): Progress | null {
  if (liveKids(s, ix, id).length === 0) return null;
  const p: Progress = { done: 0, total: 0, in_review: 0, doing: 0, blocked: 0 };
  const walk = (parent: TaskId) => {
    for (const k of liveKids(s, ix, parent)) {
      if (liveKids(s, ix, k).length) {
        walk(k);
        continue;
      }
      const st = s.tasks.get(k)!.status;
      p.total++;
      if (st === 'done') p.done++;
      else if (st === 'in_review') p.in_review++;
      else if (st === 'doing') p.doing++;
      else if (st === 'blocked') p.blocked++;
    }
  };
  walk(id);
  return p;
}

export function reposOf(s: MockState, ix: TreeIndex, id: TaskId): { repos: RepoId[]; fromSubtasks: boolean } {
  const own = s.tasks.get(id)!.repos;
  if (own.length) return { repos: [...own], fromSubtasks: false };
  const seen = new Set<RepoId>();
  for (const k of subtree(ix, id)) {
    const t = s.tasks.get(k)!;
    if (t.status === 'dropped') continue;
    for (const r of t.repos) seen.add(r);
  }
  const ordered = s.repos
    .filter((r) => seen.has(r.id))
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((r) => r.id);
  return { repos: ordered, fromSubtasks: ordered.length > 0 };
}

export function branchOf(s: MockState, id: TaskId): string | null {
  let t: Task | undefined = s.tasks.get(id);
  while (t) {
    if (t.branch) return t.branch;
    t = t.parent === null ? undefined : s.tasks.get(t.parent);
  }
  return null;
}

export function isReady(s: MockState, ix: TreeIndex, id: TaskId): boolean {
  const t = s.tasks.get(id)!;
  return t.status === 'todo' && stepsOf(s, ix, id).length === 0 && openBlockers(s, ix, id).length === 0;
}

const CHIP_RANK: Record<Chip, number> = { waiting_on_you: 4, working: 3, your_turn: 2, stopped: 1 };

export function chipOf(state: MockThread['view']['state']): Chip | null {
  switch (state) {
    case 'running':
      return 'working';
    case 'needs_me':
      return 'waiting_on_you';
    case 'idle':
      return 'your_turn';
    case 'errored':
    case 'settled':
      return 'stopped';
    case 'gone':
      return null;
  }
}

export function threadsFor(s: MockState, id: TaskId): MockThread[] {
  return [...s.threads.values()].filter((t) => t.tasks.includes(id));
}

/** Where the task's most recently active T3 thread or Herdr terminal runs (Launch's avatar). */
function launchedIn(s: MockState, id: TaskId): Source | null {
  let best: { at: string; source: Source } | null = null;
  for (const t of threadsFor(s, id)) {
    const source = t.view.key.source;
    if (source !== 't3' && source !== 'herdr') continue;
    const at = t.view.last_activity_at ?? '';
    if (!best || at > best.at) best = { at, source };
  }
  return best?.source ?? null;
}

function liveOf(s: MockState, id: TaskId): Chip | null {
  if (!isOpen(s.tasks.get(id)!.status)) return null;
  let best: Chip | null = null;
  for (const t of threadsFor(s, id)) {
    const c = chipOf(t.view.state);
    if (c && (!best || CHIP_RANK[c] > CHIP_RANK[best])) best = c;
  }
  return best;
}

export function visibleNeeds(s: MockState): NeedsItem[] {
  return s.needs.filter((n) => !s.hiddenNeeds.has(n.key));
}

function lastEvent(s: MockState, ids: Set<TaskId>): { at: Timestamp; actor: string } {
  let best = { at: '', actor: 'me' };
  for (const e of s.events) {
    if (ids.has(e.task) && e.at > best.at) best = { at: e.at, actor: e.actor };
  }
  if (!best.at) {
    for (const id of ids) {
      const t = s.tasks.get(id);
      if (t && t.created_at > best.at) best = { at: t.created_at, actor: 'me' };
    }
  }
  return best;
}

export function buildRow(s: MockState, ix: TreeIndex, id: TaskId, needs: NeedsItem[]): RowDto {
  const t = s.tasks.get(id)!;
  const waiting = isWaiting(s, ix, id);
  const { repos, fromSubtasks } = reposOf(s, ix, id);
  const ready = isReady(s, ix, id);
  const mine = needs.filter((n) => n.task === id);
  const quiet = mine.find((n) => n.kind === 'quiet');
  const progress = progressOf(s, ix, id);
  const last = progress ? lastEvent(s, new Set([id, ...subtree(ix, id)])) : lastEvent(s, new Set([id]));
  return {
    id,
    parent: t.parent,
    depth: ix.depth.get(id) ?? 0,
    position: t.position,
    title: t.title,
    status: t.status,
    origin: t.origin,
    priority: t.priority,
    blocked_reason: t.blocked_reason,
    waiting,
    waits_on: waiting ? openBlockers(s, ix, id) : [],
    has_children: progress !== null,
    has_steps: stepsOf(s, ix, id).length > 0,
    progress,
    repos,
    repos_from_subtasks: fromSubtasks,
    branch: branchOf(s, id),
    due_at: t.due_at,
    ready,
    ready_since: ready ? (s.readySince[id] ?? last.at) : null,
    ready_pill: mine.some((n) => n.kind === 'ready'),
    quiet_days: quiet ? Math.max(1, Math.floor((s.nowMs - Date.parse(quiet.since)) / DAY)) : null,
    needs: mine.length > 0,
    needs_stale: mine.some((n) => n.stale),
    live: liveOf(s, id),
    last_at: last.at,
    last_actor: last.actor,
    launched_in: last.actor === 'launch' ? launchedIn(s, id) : null,
  };
}

export function buildRows(s: MockState): { rows: Map<TaskId, RowDto>; order: TaskId[]; ix: TreeIndex } {
  rollUp(s);
  const ix = indexTree(s);
  const needs = visibleNeeds(s);
  const rows = new Map<TaskId, RowDto>();
  for (const id of ix.order) rows.set(id, buildRow(s, ix, id, needs));
  return { rows, order: ix.order, ix };
}

/** `diff(old, new)`: whole rows for what changed, ids for what's gone, order and repos only when changed. */
export function diff(
  seq: number,
  before: { rows: Map<TaskId, RowDto>; order: TaskId[]; repos: Repo[] },
  after: { rows: Map<TaskId, RowDto>; order: TaskId[]; repos: Repo[] },
  created: TaskId[] = [],
): Patch {
  const upserts: RowDto[] = [];
  for (const [id, row] of after.rows) {
    const old = before.rows.get(id);
    if (!old || JSON.stringify(old) !== JSON.stringify(row)) upserts.push(row);
  }
  const removed = [...before.rows.keys()].filter((id) => !after.rows.has(id));
  const sameOrder =
    before.order.length === after.order.length && before.order.every((id, i) => id === after.order[i]);
  const sameRepos = JSON.stringify(before.repos) === JSON.stringify(after.repos);
  return {
    seq,
    upserts,
    removed,
    order: sameOrder ? null : [...after.order],
    repos: sameRepos ? null : structuredClone(after.repos),
    created,
  };
}

// ---------------------------------------------------------------- run map and waves

/**
 * Display waves for a parent: its direct subtasks that aren't dropped or proposed (done ones
 * included), dependencies lifted to them, Kahn layering in sibling order, outside blockers,
 * and loops. A subtask that waits outside sits in the last wave (DESIGN's Refunds example:
 * waves of 3, 2, 1 and 2).
 */
export function buildRunMap(s: MockState, ix: TreeIndex, parent: TaskId): RunMap {
  const nodes = kids(ix, parent).filter((k) => {
    const st = s.tasks.get(k)!.status;
    return st !== 'dropped' && st !== 'proposed';
  });
  const top = new Map<TaskId, TaskId>();
  for (const n of nodes) {
    top.set(n, n);
    for (const d of subtree(ix, n)) top.set(d, n);
  }
  const inParent = new Set([parent, ...subtree(ix, parent)]);
  const lifted = new Map<string, RunEdge>();
  const outside = new Map<TaskId, Set<TaskId>>();
  for (const [task, on] of s.edges) {
    const to = top.get(task);
    if (to === undefined) continue;
    const from = top.get(on);
    if (from !== undefined && from !== to) {
      const key = `${from}>${to}`;
      const done = s.tasks.get(on)!.status === 'done';
      const prev = lifted.get(key);
      lifted.set(key, { from, to, done: prev ? prev.done && done : done });
    } else if (from === undefined && !inParent.has(on) && isOpen(s.tasks.get(on)!.status)) {
      outside.set(to, (outside.get(to) ?? new Set()).add(on));
    }
  }
  const edges = [...lifted.values()].sort((a, b) => a.to - b.to || a.from - b.from);
  const tangled = loops(nodes, edges);
  const wave = new Map<TaskId, number>();
  const ins = (n: TaskId) => edges.filter((e) => e.to === n && !(tangled.has(e.from) && tangled.has(e.to)));
  let left = [...nodes];
  for (let guard = 0; left.length && guard < nodes.length + 1; guard++) {
    const placed = left.filter((n) => ins(n).every((e) => wave.has(e.from)));
    if (!placed.length) break;
    for (const n of placed) wave.set(n, Math.max(0, ...ins(n).map((e) => wave.get(e.from)! + 1)));
    left = left.filter((n) => !wave.has(n));
  }
  for (const n of left) wave.set(n, 0);
  let last = Math.max(0, ...wave.values());
  for (const n of outside.keys()) wave.set(n, Math.max(wave.get(n)!, last));
  last = Math.max(0, ...wave.values());
  const waves: TaskId[][] = nodes.length ? Array.from({ length: last + 1 }, () => []) : [];
  for (const n of nodes) waves[wave.get(n)!]!.push(n);
  return {
    nodes,
    edges,
    waves,
    outside: [...outside].map(([task, b]) => ({ task, blockers: [...b].sort((x, y) => x - y) })),
    tangled: [...tangled].sort((a, b) => a - b),
  };
}

/** Nodes on a dependency cycle (strongly connected components larger than one). */
function loops(nodes: TaskId[], edges: RunEdge[]): Set<TaskId> {
  const out = new Set<TaskId>();
  const reach = (from: TaskId, target: TaskId) => {
    const seen = new Set<TaskId>();
    const stack = [from];
    while (stack.length) {
      const n = stack.pop()!;
      for (const e of edges) {
        if (e.from !== n) continue;
        if (e.to === target) return true;
        if (!seen.has(e.to)) {
          seen.add(e.to);
          stack.push(e.to);
        }
      }
    }
    return false;
  };
  for (const n of nodes) if (reach(n, n)) out.add(n);
  return out;
}

// ---------------------------------------------------------------- detail

function threadDto(t: MockThread): ThreadDto {
  return {
    key: t.view.key,
    state: t.view.state,
    chip: chipOf(t.view.state),
    title: t.view.title,
    request: t.view.request,
    error: t.view.error,
    project_path: t.view.project_path,
    url: t.url,
    last_activity_at: t.view.last_activity_at,
    last_reply: t.view.last_reply ?? null,
    stale: t.view.stale,
  };
}

function dep(s: MockState, ix: TreeIndex, id: TaskId): DepDto {
  const t = s.tasks.get(id)!;
  return { id, title: t.title, status: t.status, waiting: isWaiting(s, ix, id) };
}

export function slug(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 32)
    .replace(/-$/, '');
}

export function prepare(s: MockState, ix: TreeIndex, id: TaskId): Prepared | null {
  const t = s.tasks.get(id)!;
  if (t.status === 'proposed' || t.status === 'done' || t.status === 'dropped') return null;
  const { repos, fromSubtasks } = reposOf(s, ix, id);
  const byId = new Map(s.repos.map((r) => [r.id, r]));
  return {
    repos: repos.map((r) => byId.get(r)!).filter(Boolean),
    from_subtasks: fromSubtasks,
    needs_choice: repos.length === 0,
    choices: structuredClone(s.repos),
    branch: branchOf(s, id) ?? `task-${id}-${slug(t.title)}`,
    research_dir: `~/Library/Application Support/Airboss/research/task-${id}`,
    waiting_on: openBlockers(s, ix, id),
    started: [],
  };
}

export function buildDetail(s: MockState, id: TaskId, req: number, limit: number): TaskDetail {
  const ix = indexTree(s);
  const task = s.tasks.get(id)!;
  const links: LinkDto[] = s.links
    .filter((l) => l.task === id)
    .map((l) => {
      const source = ({ t3_thread: 't3', claude_session: 'claude', herdr_terminal: 'herdr' } as const)[l.kind as string];
      const thread = source ? s.threads.get(`${source}:${l.ref}`) : undefined;
      return {
        ...structuredClone(l),
        pr: l.kind === 'pr' ? structuredClone(s.prs.get(l.ref) ?? null) : null,
        thread: thread ? threadDto(thread) : null,
      };
    });
  const inTree = new Set([id, ...subtree(ix, id)]);
  const titles = new Map([...s.threads.values()].map((t) => [t.view.key.ref, t.view.title]));
  const all = s.events.filter((e) => inTree.has(e.task)).sort((a, b) => b.at.localeCompare(a.at) || b.id - a.id);
  const history: HistoryEntry[] = all.slice(0, limit).map((e) => ({
    ...e,
    thread_title: e.actor.startsWith('agent:') ? (titles.get(e.actor.slice(6)) ?? null) : null,
  }));
  const hasKids = liveKids(s, ix, id).length > 0;
  const branch = branchOf(s, id);
  return {
    req,
    task: structuredClone(task),
    links,
    history,
    history_more: all.length > limit,
    waits_on: (ix.blockers.get(id) ?? []).slice().sort((a, b) => a - b).map((b) => dep(s, ix, b)),
    blocks: (ix.dependents.get(id) ?? []).slice().sort((a, b) => a - b).map((b) => dep(s, ix, b)),
    waves: hasKids ? buildRunMap(s, ix, id) : null,
    proposals: subtree(ix, id)
      .filter((p) => p !== id && s.tasks.get(p)?.status === 'proposed')
      .sort((a, b) => a - b)
      .map((p) => {
        const made = s.events.filter((e) => e.task === p).sort((a, b) => a.id - b.id)[0];
        return {
          id: p,
          parent: s.tasks.get(p)!.parent!,
          by: made?.actor ?? 'me',
          at: made?.at ?? s.tasks.get(p)!.created_at,
          follows: (ix.blockers.get(p) ?? []).slice().sort((a, b) => a - b),
          blocks: (ix.dependents.get(p) ?? []).slice().sort((a, b) => a - b),
        };
      }),
    needs: visibleNeeds(s).filter((n) => n.task === id),
    prepared: prepare(s, ix, id),
    suggested_prs: [...s.prs.values()].filter(
      (p) => !p.linked && p.state === 'open' && branch !== null && p.head_ref === branch,
    ),
  };
}

// ---------------------------------------------------------------- plans and quick add

export function buildPlans(s: MockState): PlanCard[] {
  const { rows, ix } = buildRows(s);
  const cards: PlanCard[] = [];
  for (const id of kids(ix, null)) {
    const t = s.tasks.get(id)!;
    const row = rows.get(id)!;
    // Rust's plan_cards: top-level, has subtasks, and not proposed, done or dropped.
    if (!row.has_children || t.status === 'proposed' || t.status === 'done' || t.status === 'dropped' || !row.progress) continue;
    const map = buildRunMap(s, ix, id);
    const current = map.waves.findIndex((w) => w.some((n) => s.tasks.get(n)!.status !== 'done'));
    const under = subtree(ix, id);
    const ready = under.filter((k) => rows.get(k)?.ready);
    cards.push({
      id,
      title: t.title,
      status: t.status,
      waiting: row.waiting,
      progress: row.progress,
      waves: map.waves.length,
      current_wave: current === -1 ? null : current,
      next_up: ready.slice(0, 2).map((k) => ({ id: k, title: s.tasks.get(k)!.title })),
      working: under.filter((k) => rows.get(k)?.live === 'working').length,
      blocked: under.filter((k) => s.tasks.get(k)!.status === 'blocked').length,
      repos: row.repos,
      last_at: row.last_at,
    });
  }
  return cards;
}

export function parentOptions(s: MockState): ParentOption[] {
  const { rows, order } = buildRows(s);
  return order
    .map((id) => rows.get(id)!)
    .filter((r) => r.status !== 'done' && r.status !== 'dropped' && r.status !== 'proposed')
    .map((r) => ({ id: r.id, title: r.title, status: r.status, waiting: r.waiting, depth: r.depth }));
}

/** Tasks an undo entry would touch: a drop takes open subtasks with it. */
export function openDescendants(s: MockState, id: TaskId): TaskId[] {
  return subtree(indexTree(s), id).filter((k) => isOpen(s.tasks.get(k)!.status));
}

export { threadId };
