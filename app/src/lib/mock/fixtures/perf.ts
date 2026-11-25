/*
  2,000 generated tasks for performance: five levels, about 1,000 dependencies and 40 live
  threads, like `examples/seed.rs --tasks 2000`. Deterministic (seeded PRNG).
*/

import type { Status, TaskId, ThreadState } from '../../types';
import type { MockSeed } from '../model';
import { DAY, HOUR, MIN, SeedBuilder } from './build';
import { CONNECTED, NOW, seedDefaults } from './design';

const WORDS = `api web cache queue auth billing search export import report invoice refund slot
booking email sms webhook token session profile settings onboarding audit metrics alert deploy
schema index migration retry backoff limit dashboard sidebar panel modal toast menu hotkey
picker calendar timezone locale theme contrast keyboard focus scroll list tree map wave plan`.split(/\s+/);
const VERBS = ['Add', 'Fix', 'Refactor', 'Test', 'Document', 'Speed up', 'Clean up', 'Split', 'Wire', 'Review'];
const STATUSES: Status[] = ['todo', 'todo', 'todo', 'todo', 'doing', 'in_review', 'blocked', 'done', 'done', 'done', 'proposed', 'dropped'];

function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

export function perfSeed(count = 2000): MockSeed {
  const r = rng(20261125);
  const pick = <T>(xs: readonly T[]): T => xs[Math.floor(r() * xs.length)]!;
  const b = new SeedBuilder(NOW);
  b.repo(3, 'airboss').repo(1, 'booking-web').repo(2, 'booking-api').repo(4, 'dotfiles');

  const ids: TaskId[] = [];
  const depthOf = new Map<TaskId, number>();
  const parents: TaskId[] = [];
  for (let i = 0; i < count; i++) {
    const id = 1000 + i;
    let parent: TaskId | null = null;
    // About a fifth are top level; the rest hang under a recent task, up to five levels deep.
    if (ids.length > 0 && r() > 0.2) {
      const candidates = parents.slice(-40).filter((p) => (depthOf.get(p) ?? 0) < 4);
      parent = candidates.length ? pick(candidates) : null;
    }
    const depth = parent === null ? 0 : (depthOf.get(parent) ?? 0) + 1;
    depthOf.set(id, depth);
    const status = pick(STATUSES);
    const repos = r() < 0.1 ? [] : r() < 0.15 ? [1, 2] : [pick([1, 2, 3, 4])];
    b.task({
      id,
      parent,
      title: `${pick(VERBS)} ${pick(WORDS)} ${pick(WORDS)}${r() < 0.1 ? ` for the ${pick(WORDS)} ${pick(WORDS)} flow` : ''}`,
      status: parent === null && status === 'proposed' ? 'todo' : status,
      repos,
      blocked: status === 'blocked' ? 'Waiting on a review' : undefined,
      created: Math.floor(r() * 30 * DAY) + HOUR,
      origin: status === 'proposed' ? 'agent' : undefined,
    });
    if (status !== 'todo' && status !== 'proposed') b.moved(id, status, Math.floor(r() * 10 * DAY) + MIN, pick(['me', 'agent:worker', 'sync:gh', 'launch']));
    ids.push(id);
    if (depth < 4 && r() < 0.35) parents.push(id);
  }
  // About 1,000 dependencies, always on an earlier task, never on an ancestor.
  const ancestorsOf = (id: TaskId) => {
    const out = new Set<TaskId>();
    let p = b.tasks[id - 1000]!.parent;
    while (p !== null) {
      out.add(p);
      p = b.tasks[p - 1000]!.parent;
    }
    return out;
  };
  let edges = 0;
  for (let guard = 0; edges < count / 2 && guard < count * 4; guard++) {
    const a = 1000 + 1 + Math.floor(r() * (count - 1));
    const on = 1000 + Math.floor(r() * (a - 1000));
    if (ancestorsOf(a).has(on) || b.edges.some(([x, y]) => x === a && y === on)) continue;
    b.waits(a, on);
    edges++;
  }
  // 40 live threads on open tasks.
  const states: ThreadState[] = ['running', 'running', 'needs_me', 'errored', 'idle'];
  let threads = 0;
  for (const t of b.tasks) {
    if (threads >= 40) break;
    if (t.status === 'doing' && r() < 0.5) {
      b.thread(t.id, `thread-${t.id}`, pick(states), { repo: 'booking-web', launchedAgo: DAY, ago: 5 * MIN });
      threads++;
    }
  }
  const seed = seedDefaults(b, 'perf', []);
  seed.connections = CONNECTED;
  return seed;
}
