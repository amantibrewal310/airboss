/*
  ⌘K Jump: fuzzy match on id and title over the rows in memory (dropped left out), plus the
  command registry. With no query: the 5 most recently changed open tasks and 4 commands; with a
  query: 8 tasks and 5 commands. A number puts that id first.
*/

import type { RowDto, TaskId } from '../types';
import { ancestorsOf } from './views';

/** Higher is better; 0 means no match. Substrings beat scattered letters; word starts and prefixes win. */
export function score(query: string, text: string): number {
  const q = query.trim().toLowerCase();
  if (!q) return 1;
  const t = text.toLowerCase();
  const at = t.indexOf(q);
  if (at !== -1) {
    const wordStart = at === 0 || /[\s:/._-]/.test(t[at - 1]!);
    return 1000 - at + (wordStart ? 200 : 0) + (at === 0 ? 100 : 0) - t.length / 100;
  }
  // Every query letter in order.
  let i = 0;
  let gaps = 0;
  let last = -1;
  for (let j = 0; j < t.length && i < q.length; j++) {
    if (t[j] === q[i]) {
      if (last !== -1) gaps += j - last - 1;
      last = j;
      i++;
    }
  }
  return i === q.length ? Math.max(1, 300 - gaps * 5) : 0;
}

export function searchTasks(rows: Map<TaskId, RowDto>, order: TaskId[], query: string): RowDto[] {
  const q = query.trim();
  const all: RowDto[] = [];
  for (const id of order) {
    const r = rows.get(id);
    if (r && r.status !== 'dropped') all.push(r);
  }
  if (!q) {
    return all
      .filter((r) => r.status !== 'done' && r.status !== 'proposed')
      .sort((a, b) => b.last_at.localeCompare(a.last_at) || b.id - a.id)
      .slice(0, 5);
  }
  const asId = /^\d+$/.test(q) ? Number(q) : null;
  const scored = all
    .map((r) => ({ r, s: r.id === asId ? 1e6 : Math.max(score(q, `${r.id} ${r.title}`), score(q, r.title)) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || a.r.id - b.r.id);
  return scored.slice(0, 8).map((x) => x.r);
}

export function searchCommands<T extends { label: string }>(commands: T[], query: string): T[] {
  const q = query.trim();
  if (!q) return commands.slice(0, 4);
  return commands
    .map((c) => ({ c, s: score(q, c.label) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, 5)
    .map((x) => x.c);
}

/** Picker mode: why a task can't be chosen, or null when it can. */
export function invalidTarget(
  rows: Map<TaskId, RowDto>,
  mode: 'waits_on' | 'move_under',
  task: TaskId,
  target: TaskId,
): string | null {
  if (target === task) return mode === 'waits_on' ? "A task can't wait on itself" : "A task can't go under itself";
  const t = rows.get(target);
  if (!t) return 'No longer exists';
  if (ancestorsOf(rows, target).includes(task)) return mode === 'waits_on' ? 'Its own subtask' : 'One of its subtasks';
  if (mode === 'waits_on') {
    if (ancestorsOf(rows, task).includes(target)) return 'Its own parent';
    return null;
  }
  if (t.status === 'proposed' || t.status === 'done' || t.status === 'dropped') {
    return `Can't go under a ${t.status} task`;
  }
  return null;
}
