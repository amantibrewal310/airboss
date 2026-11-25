/*
  The mock's stand-in for `attention`: after a write it drops items whose reason has gone and
  adds the Ready and Finish it? items a write can cause, as SPEC-attention's table describes.
*/

import type { NeedsGroup, NeedsItem, TaskId } from '../types';
import { isOpen, ts, type MockState } from './model';
import { indexTree, isReady, liveKids, openBlockers, visibleNeeds, type TreeIndex } from './viewmodel';

export const GROUP_ORDER: NeedsGroup[] = ['questions', 'reviews', 'failed_runs', 'stopped', 'proposed', 'ready', 'finish_it', 'quiet'];

export function sortNeeds(items: NeedsItem[]): NeedsItem[] {
  return [...items].sort(
    (a, b) =>
      GROUP_ORDER.indexOf(a.group) - GROUP_ORDER.indexOf(b.group) ||
      (a.group === 'quiet' ? a.since.localeCompare(b.since) : b.since.localeCompare(a.since)) ||
      a.task - b.task ||
      a.key.localeCompare(b.key),
  );
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
      // A doing task whose every linked thread stopped, with no open PR (SPEC-attention).
      if (t.status !== 'doing') return null;
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
    case 'ready':
      return isReady(s, ix, n.task) ? n : null;
    case 'finish_it':
      return finishable(s, ix, n.task) ? n : null;
    case 'quiet':
      return t.status === 'doing' || t.status === 'in_review' || t.status === 'blocked' ? n : null;
  }
}

function finishable(s: MockState, ix: TreeIndex, id: TaskId): boolean {
  const t = s.tasks.get(id)!;
  const kids = liveKids(s, ix, id);
  return isOpen(t.status) && kids.length > 0 && kids.every((k) => s.tasks.get(k)!.status === 'done');
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
    if (finishable(s, ix, t.id) && !has('finish_it', t.id)) {
      const n = liveKids(s, ix, t.id).length;
      kept.push({
        key: `finish:${t.id}:${now}`,
        kind: 'finish_it',
        group: 'finish_it',
        task: t.id,
        task_title: t.title,
        since: now,
        headline: `All ${n} subtasks are done. Mark the parent done?`,
        context: `All ${n} subtasks done`,
        target: { task: t.id },
        proposed: [],
        stale: false,
      });
    }
    if (!isReady(s, ix, t.id) && openBlockers(s, ix, t.id).length) delete s.readySince[t.id];
  }
  for (const n of kept) n.task_title = s.tasks.get(n.task)?.title ?? n.task_title;
  s.needs = sortNeeds(kept);
  const changed = JSON.stringify(visibleNeeds(s)) !== before;
  if (changed) version.value++;
  return changed;
}
