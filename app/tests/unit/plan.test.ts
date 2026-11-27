import { describe, expect, it } from 'vitest';
import { needsUnder, planSections, planSummary, proposalEdges, proposalTurns, subtreeOf } from '../../src/lib/logic/plan';
import type { NeedsItem, ProposalDto, RowDto, RunMap } from '../../src/lib/types';

const r = (id: number, over: Partial<RowDto> = {}): RowDto => ({
  id, parent: 1, depth: 1, position: id, title: `T${id}`, status: 'todo', origin: 'plan', blocked_reason: null,
  waiting: false, waits_on: [], has_children: false, has_steps: false, progress: null, repos: [1], repos_from_subtasks: false,
  branch: null, due_at: null, priority: null, ready: false, ready_since: null, needs: false,
  live: null, signal: null, last_at: '2026-11-26T10:00:00.000Z', last_actor: 'plan', ...over,
});
const phase = { has_children: true, has_steps: true };

describe('plan sections', () => {
  // 1 › 2 done · 3 doing · 4 (waits on 3) · 5 ready · 6 phase waiting on 3 (7 ready but held) · 8 proposed · 9 dropped · 10 in review
  const list = [
    r(1, { parent: null, depth: 0, ...phase, status: 'doing' }),
    r(2, { status: 'done' }),
    r(3, { status: 'doing' }),
    r(4, { waiting: true, waits_on: [3] }),
    r(5, { ready: true }),
    r(6, { ...phase, waiting: true, waits_on: [3] }),
    r(7, { parent: 6, depth: 2, ready: true }),
    r(8, { status: 'proposed', origin: 'agent' }),
    r(9, { status: 'dropped' }),
    r(10, { status: 'in_review' }),
  ];
  const rows = new Map(list.map((x) => [x.id, x]));
  const order = list.map((x) => x.id);
  // Run order puts 5 first, then 2 and 3, then 4, 6 and 10.
  const map: RunMap = { nodes: [2, 3, 4, 5, 6, 10], edges: [], waves: [[5], [2, 3], [4, 6, 10]], outside: [], tangled: [] };

  it('puts each leaf step in one section, in run order, and leaves out proposals and drops', () => {
    expect(planSections(rows, order, 1, map)).toEqual({ ready: [5], doing: [3, 10], next: [4, 7], done: [2] });
  });

  it('falls back to tree order without a map', () => {
    expect(planSections(rows, order, 1, null).doing).toEqual([3, 10]);
  });

  it('never lists a finished step as ready', () => {
    const done = new Map(rows).set(5, r(5, { status: 'done', ready: false }));
    expect(planSections(done, order, 1, map).ready).toEqual([]);
  });

  it('finds the subtree', () => {
    expect([...subtreeOf(rows, order, 6)]).toEqual([6, 7]);
  });
});

describe('plan summary', () => {
  it('says done, ready, to review and blocked, skipping zeros', () => {
    expect(planSummary({ done: 6, total: 19, in_review: 4, doing: 2, blocked: 0 }, 2)).toBe('6 of 19 done · 2 ready · 4 to review');
    expect(planSummary({ done: 0, total: 3, in_review: 0, doing: 0, blocked: 1 }, 0)).toBe('0 of 3 done · 1 blocked');
  });
});

describe('needs under a parent', () => {
  const item = (task: number, kind: NeedsItem['kind']): NeedsItem => ({
    key: `${kind}:${task}`, kind, group: 'questions', task, task_title: '', since: '', headline: '', context: '',
    target: { task }, proposed: [], stale: false,
  });
  it('keeps the subtree, without Ready (Ready now has them) and proposals (listed one by one)', () => {
    const items = [item(3, 'question'), item(5, 'ready'), item(1, 'proposed'), item(99, 'review'), item(1, 'quiet')];
    expect(needsUnder(items, new Set([1, 3, 5])).map((n) => n.key)).toEqual(['question:3', 'quiet:1']);
  });
});

describe('proposal turns', () => {
  const p = (id: number, by: string, at: string, parent = 1): ProposalDto => ({ id, parent, by, at: `2026-11-27T10:${at}.000Z`, follows: [], blocks: [] });

  it('groups one agent’s proposals made seconds apart under one parent', () => {
    const turns = proposalTurns([p(11, 'agent:t3/a', '00:01'), p(12, 'agent:t3/a', '00:02'), p(13, 'agent:t3/a', '00:03')]);
    expect(turns.map((t) => t.map((x) => x.id))).toEqual([[11, 12, 13]]);
  });

  it('splits by agent, by parent and by a long gap', () => {
    const turns = proposalTurns([
      p(11, 'agent:t3/a', '00:01'),
      p(12, 'agent:t3/b', '00:02'),
      p(13, 'agent:t3/a', '00:03', 2),
      p(14, 'agent:t3/a', '00:04'),
      p(15, 'agent:t3/a', '30:00'),
    ]);
    expect(turns.map((t) => t.map((x) => x.id))).toEqual([[11, 14], [12], [13], [15]]);
  });

  it('says what a proposal follows and would block', () => {
    expect(proposalEdges({ follows: [12], blocks: [20, 21] })).toBe('follows #12 · would block #20, #21');
    expect(proposalEdges({ follows: [], blocks: [] })).toBe('');
  });
});
