/*
  The mock's stand-in for `attention`: after a write it drops items whose reason has gone and
  adds the Ready items a write can cause, as SPEC-attention's table describes.
*/

import type { NeedsGroup, NeedsItem, Task, TaskId } from '../types';
import { dueDay, dueLabel } from '../logic/time';
import { isOpen, ts, type MockState } from './model';
import { indexTree, isReady, liveKids, openBlockers, stepsOf, visibleNeeds, type TreeIndex } from './viewmodel';

export const GROUP_ORDER: NeedsGroup[] = ['questions', 'reviews', 'failed_runs', 'stopped', 'due', 'proposed', 'ready', 'quiet'];

export function sortNeeds(items: NeedsItem[]): NeedsItem[] {
  return [...items].sort(
    (a, b) =>
      GROUP_ORDER.indexOf(a.group) - GROUP_ORDER.indexOf(b.group) ||
      (a.group === 'quiet' ? a.since.localeCompare(b.since) : b.since.localeCompare(a.since)) ||
      a.task - b.task ||
      a.key.localeCompare(b.key),
  );
}

const DAY = 86_400_000;

/** A Due item for an open task whose date has come, worded as `attention` words it; else null. */
export function dueItem(task: { id: TaskId; title: string; status: Task['status']; due_at: string | null }, nowMs: number): NeedsItem | null {
  const day = dueDay(task.due_at);
  if (day === null || day > nowMs || !isOpen(task.status) || task.status === 'proposed') return null;
  const start = new Date(nowMs);
  const today = new Date(start.getFullYear(), start.getMonth(), start.getDate()).getTime();
  const late = Math.round((today - day) / DAY);
  const headline = late <= 0 ? 'Due today' : late === 1 ? 'Due yesterday' : `Overdue by ${late} days`;
  return {
    key: `due:${task.id}:${task.due_at}`,
    kind: 'due',
    group: 'due',
    task: task.id,
    task_title: task.title,
    since: new Date(day).toISOString(),
    headline,
    context: `Due ${dueLabel(task.due_at!, nowMs)}`,
    target: { task: task.id },
    proposed: [],
    stale: false,
  };
}

function stillHolds(s: MockState, ix: TreeIndex, n: NeedsItem): NeedsItem | null {
  const t = s.tasks.get(n.task);
  if (!t) return null;
  switch (n.kind) {
    case 'question':
    case 'approval':
    case 'failed_run': {
      if (!isOpen(t.status) || !('thread' in n.target)) return null;
      const th = s.threads.get(`${n.target.thread.source}:${n.target.thread.ref}`);
      const want = n.kind === 'failed_run' ? 'errored' : 'needs_me';
      return th && th.view.state === want ? n : null;
    }
    case 'agent_stopped': {
      // A doing task whose every linked thread stopped, with no open PR (SPEC-attention). Never
      // a parent: its status follows its steps (SPEC-core rule 10).
      if (t.status !== 'doing' || stepsOf(s, ix, n.task).length) return null;
      const threads = [...s.threads.values()].filter((th) => th.tasks.includes(n.task));
      const stopped = threads.length > 0 && threads.every((th) => ['idle', 'settled', 'gone'].includes(th.view.state));
      const openPr = [...s.prs.values()].some((p) => p.state === 'open' && s.links.some((l) => l.task === n.task && l.kind === 'pr' && l.ref === p.ref));
      return stopped && !openPr ? n : null;
    }
    case 'review': {
      if (!isOpen(t.status) || !('pr' in n.target)) return null;
      const pr = s.prs.get(n.target.pr.ref);
      return pr && pr.state === 'open' ? n : null;
    }
    case 'proposed': {
      const proposed = liveKids(s, ix, n.task)
        .map((k) => s.tasks.get(k)!)
        .filter((k) => k.status === 'proposed')
        .map((k): [TaskId, string] => [k.id, k.title]);
      if (!proposed.length) return null;
      const count = proposed.length;
      return {
        ...n,
        proposed,
        headline: `The agent proposed ${count} subtask${count === 1 ? '' : 's'}`,
        context: `${count} proposed subtask${count === 1 ? '' : 's'}`,
      };
    }
    case 'due':
      return n.key === `due:${t.id}:${t.due_at}` ? dueItem(t, s.nowMs) : null;
    case 'ready':
      return isReady(s, ix, n.task) ? n : null;
    case 'quiet':
      return t.status === 'doing' || t.status === 'in_review' || t.status === 'blocked' ? n : null;
  }
}

/** Recomputes the list. Returns true when it changed (and bumps `version`). */
export function recomputeNeeds(s: MockState, version: { value: number }): boolean {
  const ix = indexTree(s);
  const before = JSON.stringify(visibleNeeds(s));
  const kept = s.needs.map((n) => stillHolds(s, ix, n)).filter((n): n is NeedsItem => n !== null);
  const has = (kind: NeedsItem['kind'], task: TaskId) => kept.some((n) => n.kind === kind && n.task === task);
  const now = ts(s.nowMs);
  for (const t of s.tasks.values()) {
    // A task that just became ready because its blockers finished.
    const blockers = ix.blockers.get(t.id) ?? [];
    if (blockers.length && isReady(s, ix, t.id) && !has('ready', t.id) && !s.readySince[t.id]) {
      s.readySince[t.id] = now;
      const done = blockers.slice().sort((a, b) => a - b);
      const names = done.length <= 2 ? done.join(' and ') : `${done.length} tasks it waited on`;
      const verb = done.length === 1 ? 'is' : 'are';
      kept.push({
        key: `ready:${t.id}:${now}`,
        kind: 'ready',
        group: 'ready',
        task: t.id,
        task_title: t.title,
        since: now,
        headline: `Ready to start: ${names} ${verb} done`,
        context: `${names} done · ready to start`,
        target: { task: t.id },
        proposed: [],
        stale: false,
      });
    }
    if (!isReady(s, ix, t.id) && openBlockers(s, ix, t.id).length) delete s.readySince[t.id];
    // A task whose due date has come, or that was given one that has.
    const due = dueItem(t, s.nowMs);
    if (due && !kept.some((n) => n.key === due.key)) kept.push(due);
  }
  for (const n of kept) n.task_title = s.tasks.get(n.task)?.title ?? n.task_title;
  s.needs = sortNeeds(kept);
  const changed = JSON.stringify(visibleNeeds(s)) !== before;
  if (changed) version.value++;
  return changed;
}
