import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MockBackend, UnscriptedCall, createReplayTransport } from '../../src/lib/mock';
import { loadFixture } from '../../src/lib/mock/fixtures';
import { buildRows, buildRunMap, indexTree } from '../../src/lib/mock/viewmodel';
import { stateFromSeed } from '../../src/lib/mock/model';
import type { CommandName } from '../../src/lib/api/contract';

const fast = { launch: 0, attention: 0, reject: 50, lookup: 0 };

function rowsOf(name: Parameters<typeof loadFixture>[0] = 'design') {
  return buildRows(stateFromSeed(loadFixture(name))).rows;
}

describe('mock view model (SPEC-app "The view model and patches")', () => {
  it('counts progress over leaf subtasks, leaving dropped out and proposed as todo', () => {
    const rows = rowsOf('full');
    expect(rows.get(142)!.progress).toEqual({ done: 1, total: 5, in_review: 0, doing: 1, blocked: 0 });
    expect(rows.get(200)!.progress).toEqual({ done: 1, total: 8, in_review: 1, doing: 1, blocked: 1 });
    // 160: 161 done; 162 → leaves 163 (doing), 164; 165; 166 → leaf 171; 167; 172 dropped.
    expect(rows.get(160)!.progress).toEqual({ done: 1, total: 6, in_review: 0, doing: 1, blocked: 0 });
    // Proposed subtasks count as todo.
    expect(rows.get(150)!.progress).toEqual({ done: 0, total: 3, in_review: 0, doing: 0, blocked: 0 });
    expect(rows.get(144)!.progress).toBeNull();
  });

  it('shows a parent with no repos the union of its subtasks, flagged', () => {
    const rows = rowsOf();
    expect(rows.get(142)!.repos).toEqual([2, 1]); // the union, by name
    expect(rows.get(142)!.repos_from_subtasks).toBe(true);
    expect(rows.get(144)!.repos).toEqual([1]);
    expect(rows.get(144)!.repos_from_subtasks).toBe(false);
  });

  it('inherits the branch from the nearest ancestor', () => {
    const rows = rowsOf();
    expect(rows.get(144)!.branch).toBe('task-142-booking-flow');
    expect(rows.get(128)!.branch).toBeNull();
  });

  it('picks the strongest live chip: waiting on you, then working, then stopped', () => {
    const rows = rowsOf();
    expect(rows.get(144)!.live).toBe('waiting_on_you');
    expect(rows.get(202)!.live).toBe('working');
    expect(rows.get(131)!.live).toBe('stopped');
    expect(rows.get(143)!.live).toBeNull();
  });

  it('marks a todo task with open blockers as waiting, listing only open ones', () => {
    const rows = rowsOf();
    expect(rows.get(145)!.waiting).toBe(true);
    expect(rows.get(145)!.waits_on).toEqual([144]);
    expect(rows.get(147)!.waits_on).toEqual([144, 145, 146]);
    expect(rows.get(146)!.waiting).toBe(false);
    expect(rows.get(146)!.ready).toBe(true);
    expect(rows.get(146)!.ready_pill).toBe(true);
  });

  it('builds DESIGN’s Refunds run map: waves of 3, 2, 1 and 2, one outside blocker', () => {
    const s = stateFromSeed(loadFixture('design'));
    const map = buildRunMap(s, indexTree(s), 200);
    expect(map.waves).toEqual([[201, 204, 206], [202, 203], [205], [207, 208]]);
    expect(map.outside).toEqual([{ task: 208, blockers: [128] }]);
    expect(map.edges).toContainEqual({ from: 201, to: 202, done: true });
    expect(map.edges).toContainEqual({ from: 204, to: 205, done: false });
    expect(map.tangled).toEqual([]);
  });

  it('lifts a subtask’s dependency to its top-level subtask and finds the loop', () => {
    const s = stateFromSeed(loadFixture('full'));
    const map = buildRunMap(s, indexTree(s), 160);
    expect(map.nodes).not.toContain(172);
    expect(map.edges).toContainEqual({ from: 167, to: 166, done: false });
    expect(map.tangled).toEqual([166, 167]);
  });

  it('generates 2,000 tasks with about 1,000 dependencies', () => {
    const seed = loadFixture('perf');
    expect(seed.tasks).toHaveLength(2000);
    expect(seed.edges.length).toBeGreaterThanOrEqual(990);
    const depth = Math.max(...[...buildRows(stateFromSeed(seed)).rows.values()].map((r) => r.depth));
    expect(depth).toBe(4);
  });
});

describe('MockBackend', () => {
  let m: MockBackend;
  beforeEach(() => {
    m = new MockBackend(loadFixture('design'), { delays: fast });
  });
  afterEach(() => vi.useRealTimers());

  it('throws UnscriptedCall for a command it does not know', async () => {
    await expect(m.invoke('drop_everything' as CommandName, {} as never)).rejects.toBeInstanceOf(UnscriptedCall);
  });

  it('returns a patch from a write and sends the same patch as tree:patch', async () => {
    const boot = await m.invoke('bootstrap', {});
    const seen: number[] = [];
    await m.listen('tree:patch', (p) => seen.push(p.seq));
    const res = await m.invoke('toggle_done', { id: 146 });
    expect(res.patch!.seq).toBe(boot.seq + 1);
    expect(res.patch!.upserts.map((r) => r.id)).toContain(146);
    // The parent's progress changes too.
    expect(res.patch!.upserts.map((r) => r.id)).toContain(142);
    expect(res.undo!.label).toBe('Mark done #146');
    await new Promise((r) => setTimeout(r, 5));
    expect(seen).toContain(res.patch!.seq);
  });

  it('refuses an empty title with the Appendix D message', async () => {
    await expect(
      m.invoke('create_task', { task: { title: '  ', body: null, parent: null, repos: [], links: [], then: 'none' } }),
    ).rejects.toMatchObject({ code: 'empty_title', message: 'A task needs a title.' });
  });

  it('refuses a loop with the Appendix D message', async () => {
    await expect(m.invoke('add_dependency', { id: 144, waitsOn: 145 })).rejects.toMatchObject({
      code: 'loop',
      message: 'That would make a loop: #145 already waits on #144.',
    });
  });

  it('asks for a choice when Do it has no repo, then starts in the chosen repo', async () => {
    const first = await m.invoke('do_it', { id: 206, place: null, again: false });
    expect('needs_choice' in first).toBe(true);
    const second = await m.invoke('do_it', { id: 206, place: { repo: 2 }, again: false });
    expect('started' in second && second.started.report.started).toHaveLength(1);
  });

  it('reports a partial failure where a repo is not a T3 project, then retries after Add to T3', async () => {
    const res = await m.invoke('do_it', { id: 151, place: null, again: false });
    if (!('started' in res)) throw new Error('expected a start');
    expect(res.started.report.started.map((t) => t.repo)).toEqual([3]);
    expect(res.started.report.failed.map(([r, e]) => [r, e.code])).toEqual([[4, 'not_a_project']]);
    await m.invoke('add_to_t3', { repo: 4 });
    const retry = await m.invoke('do_it', { id: 151, place: null, again: false });
    if (!('started' in retry)) throw new Error('expected a start');
    expect(retry.started.report.skipped).toEqual([3]);
    expect(retry.started.report.started.map((t) => t.repo)).toEqual([4]);
  });

  it('defers a reject for the undo window and cancels it on undo', async () => {
    const res = await m.invoke('reject_proposals', { ids: [153], reason: null });
    expect(res.patch).toBeNull();
    await m.invoke('undo', { entry: res.undo!.id });
    await new Promise((r) => setTimeout(r, 80));
    expect(m.s.tasks.get(153)!.status).toBe('proposed');
    const again = await m.invoke('reject_proposals', { ids: [154], reason: null });
    expect(again.undo).not.toBeNull();
    await new Promise((r) => setTimeout(r, 80));
    expect(m.s.tasks.get(154)!.status).toBe('dropped');
  });

  it('undoes a drop, restoring the subtree’s previous statuses', async () => {
    const res = await m.invoke('set_status', { id: 142, status: 'dropped', note: null });
    expect(m.s.tasks.get(144)!.status).toBe('dropped');
    expect(m.s.tasks.get(143)!.status).toBe('done');
    const undone = await m.invoke('undo', { entry: res.undo!.id });
    expect(undone.message).toBe('Undid drop #142');
    expect(m.s.tasks.get(142)!.status).toBe('doing');
    expect(m.s.tasks.get(144)!.status).toBe('doing');
    expect(m.s.tasks.get(145)!.status).toBe('todo');
  });

  it('drops the Ready item once attention follows a Do it', async () => {
    let latest = 0;
    await m.listen('needs:changed', (n) => (latest = n.count));
    await m.invoke('do_it', { id: 146, place: null, again: false });
    await new Promise((r) => setTimeout(r, 20));
    expect(latest).toBe(7);
  });
});

describe('replay transport', () => {
  const rec = {
    reads: { quickadd_data: { repos: [], parents: [] } },
    script: [
      { command: 'rename_task', args: { id: 1, title: 'A' }, response: { seq: 2 }, events: [{ event: 'tree:patch', payload: { seq: 2 } }] },
      { command: 'add_note', args: { id: 1, text: 'n' }, response: { seq: 3 }, events: [] },
    ],
  };

  it('answers reads any number of times and writes in order', async () => {
    const t = createReplayTransport(rec);
    const got: unknown[] = [];
    await t.listen('tree:patch', (p) => got.push(p));
    await t.invoke('quickadd_data', {});
    await t.invoke('quickadd_data', {});
    expect(await t.invoke('rename_task', { title: 'A', id: 1 })).toEqual({ seq: 2 });
    await new Promise((r) => setTimeout(r, 0));
    expect(got).toEqual([{ seq: 2 }]);
    expect(t.remaining()).toBe(1);
  });

  it('throws UnscriptedCall for a write out of order or with other args', async () => {
    const t = createReplayTransport(rec);
    await expect(t.invoke('add_note', { id: 1, text: 'n' })).rejects.toBeInstanceOf(UnscriptedCall);
    await expect(t.invoke('rename_task', { id: 1, title: 'B' })).rejects.toBeInstanceOf(UnscriptedCall);
  });
});
