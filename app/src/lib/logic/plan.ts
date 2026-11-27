/*
  The parent panel as decisions (task 54): one summary line, then what needs me, what can start,
  what's under way, what's next and what's done. Pure, so it's unit-tested without the UI.
*/

import type { NeedsItem, Progress, ProposalDto, RowDto, RunMap, TaskId } from '../types';
import { readySteps } from './launch';

/** A parent's steps by what they ask of me, each list in run order. */
export interface PlanSections {
  /** What Start ready would start now. */
  ready: TaskId[];
  /** Under way: in progress, in review or blocked. */
  doing: TaskId[];
  /** Open, but not startable yet: waiting, or held back by a phase that waits. */
  next: TaskId[];
  done: TaskId[];
}

/**
 * The leaf steps under `parent` at any depth (the ones progress counts), sorted into sections.
 * Proposals and dropped tasks, and everything under them, are left out: proposals show under
 * Needs you. Run order is the display wave of the step's top-level subtask, then tree order.
 */
export function planSections(rows: Map<TaskId, RowDto>, order: TaskId[], parent: TaskId, map: RunMap | null): PlanSections {
  const kids = new Map<TaskId, TaskId[]>();
  for (const id of order) {
    const p = rows.get(id)?.parent;
    if (p != null) kids.set(p, [...(kids.get(p) ?? []), id]);
  }
  const wave = new Map<TaskId, number>();
  map?.waves.forEach((ids, w) => ids.forEach((id) => wave.set(id, w)));
  const place = new Map(order.map((id, i) => [id, i]));
  const rank = new Map<TaskId, number>();

  const leaves: TaskId[] = [];
  const walk = (id: TaskId, top: TaskId | null) => {
    for (const c of kids.get(id) ?? []) {
      const r = rows.get(c);
      if (!r || r.status === 'proposed' || r.status === 'dropped') continue;
      const station = top ?? c;
      if (r.has_steps) walk(c, station);
      else {
        leaves.push(c);
        rank.set(c, wave.get(station) ?? Number.MAX_SAFE_INTEGER);
      }
    }
  };
  walk(parent, null);
  leaves.sort((a, b) => rank.get(a)! - rank.get(b)! || place.get(a)! - place.get(b)!);

  const startable = new Set(readySteps(rows, order, parent));
  const out: PlanSections = { ready: [], doing: [], next: [], done: [] };
  for (const id of leaves) {
    const s = rows.get(id)!.status;
    if (s === 'done') out.done.push(id);
    else if (startable.has(id)) out.ready.push(id);
    else if (s === 'doing' || s === 'in_review' || s === 'blocked') out.doing.push(id);
    else out.next.push(id);
  }
  return out;
}

/** "6 of 19 done · 2 ready · 4 to review · 1 blocked", leaving out the parts that are zero. */
export function planSummary(p: Progress, ready: number): string {
  const parts = [`${p.done} of ${p.total} done`];
  if (ready) parts.push(`${ready} ready`);
  if (p.in_review) parts.push(`${p.in_review} to review`);
  if (p.blocked) parts.push(`${p.blocked} blocked`);
  return parts.join(' · ');
}

/** `id` and every task under it. */
export function subtreeOf(rows: Map<TaskId, RowDto>, order: TaskId[], id: TaskId): Set<TaskId> {
  const inside = new Set([id]);
  // Tree order lists a parent before its subtasks, so one pass finds them all.
  for (const t of order) {
    const p = rows.get(t)?.parent;
    if (p != null && inside.has(p)) inside.add(t);
  }
  return inside;
}

/**
 * The Needs me items about the parent or anything under it, except Ready to start (Ready now
 * lists those steps) and proposals (shown one by one, see `proposalTurns`).
 */
export function needsUnder(items: NeedsItem[], inside: Set<TaskId>): NeedsItem[] {
  return items.filter((n) => inside.has(n.task) && n.kind !== 'ready' && n.kind !== 'proposed');
}

/** Proposals made by the same agent under the same parent this close together came from one turn. */
export const TURN_GAP_MS = 2 * 60 * 1000;

/**
 * Open proposals in groups, one group per agent turn: the same agent, the same parent, each
 * made within `TURN_GAP_MS` of the one before. `propose_tasks` writes a turn's proposals one
 * after another, so they land seconds apart. Groups come in id order of their first proposal.
 */
export function proposalTurns(proposals: ProposalDto[]): ProposalDto[][] {
  const sorted = proposals.slice().sort((a, b) => a.at.localeCompare(b.at) || a.id - b.id);
  const open = new Map<string, { turn: ProposalDto[]; last: number }>();
  const turns: ProposalDto[][] = [];
  for (const p of sorted) {
    const key = `${p.parent}\u0000${p.by}`;
    const at = Date.parse(p.at);
    const cur = open.get(key);
    if (cur && !Number.isNaN(at) && at - cur.last <= TURN_GAP_MS) {
      cur.turn.push(p);
      cur.last = at;
    } else {
      const turn = [p];
      turns.push(turn);
      open.set(key, { turn, last: at });
    }
  }
  for (const t of turns) t.sort((a, b) => a.id - b.id);
  return turns.sort((a, b) => a[0]!.id - b[0]!.id);
}

/** "follows #12, #13 · would block #20", or '' when neither is known. */
export function proposalEdges(p: Pick<ProposalDto, 'follows' | 'blocks'>): string {
  const ids = (list: TaskId[]) => list.map((id) => `#${id}`).join(', ');
  const parts: string[] = [];
  if (p.follows.length) parts.push(`follows ${ids(p.follows)}`);
  if (p.blocks.length) parts.push(`would block ${ids(p.blocks)}`);
  return parts.join(' · ');
}
