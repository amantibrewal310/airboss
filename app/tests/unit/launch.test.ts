import { describe, expect, it } from 'vitest';
import { launchToast, readySteps, startReadyToast } from '../../src/lib/logic/launch';
import type { LaunchReport, Repo, RowDto, StartedThread } from '../../src/lib/types';

const repos = new Map<number, Repo>([
  [1, { id: 1, name: 'booking-web', path: '', remote_url: null }],
  [2, { id: 2, name: 'booking-api', path: '', remote_url: null }],
]);
const thread = (repo: number | null): StartedThread => ({ key: { source: 't3', ref: `t${repo}` }, mode: 'do_it', repo, title: 'x', linked: true });
const report = (r: Partial<LaunchReport>): LaunchReport => ({ started: [], skipped: [], failed: [], branch: 'task-142-booking-flow', moved_to_doing: true, ...r });

/** SPEC-app "Do it and Plan it results": every row of the table. */
describe('launch toasts', () => {
  it('one thread started', () => {
    expect(launchToast(142, 'do_it', report({ started: [thread(1)] }), repos)).toMatchObject({
      text: 'Started a thread for #142 in booking-web',
      actions: [{ kind: 'open_thread' }],
    });
  });
  it('several, all started', () => {
    expect(launchToast(142, 'do_it', report({ started: [thread(1), thread(2)] }), repos)).toMatchObject({
      text: 'Started 2 threads for #142 on task-142-booking-flow',
      actions: [],
    });
  });
  it('research only', () => {
    expect(launchToast(142, 'do_it', report({ started: [thread(null)] }), repos).text).toBe('Started a research thread for #142');
  });
  it('Plan it', () => {
    expect(launchToast(142, 'plan_it', report({ started: [thread(1)] }), repos).text).toBe(
      'Started a planning thread for #142. Its steps become subtasks when you settle the plan.',
    );
  });
  it('some started, some failed: Retry, or Add to T3 and retry for NotAProject', () => {
    const t = launchToast(142, 'do_it', report({ started: [thread(1)], failed: [[2, { code: 'not_a_project', message: "booking-api isn't a T3 project yet." }]] }), repos);
    expect(t.text).toBe("Started in booking-web. booking-api failed: booking-api isn't a T3 project yet.");
    expect(t.tone).toBe('warn');
    expect(t.actions).toEqual([{ kind: 'add_to_t3', label: 'Add to T3 and retry', repo: 2 }]);
    const other = launchToast(142, 'do_it', report({ started: [thread(1)], failed: [[2, { code: 't3_failed', message: 'boom' }]] }), repos);
    expect(other.actions).toEqual([{ kind: 'retry', label: 'Retry' }]);
  });
  it('none started, all skipped: Start again', () => {
    expect(launchToast(142, 'do_it', report({ skipped: [1, 2] }), repos)).toMatchObject({
      text: '#142 already has threads in booking-web and booking-api',
      actions: [{ kind: 'again' }],
    });
  });
  it('none started, failed: Retry', () => {
    expect(launchToast(142, 'do_it', report({ failed: [[null, { code: 't3_not_running', message: "T3 Code isn't open. Open it, then try again." }]] }), repos)).toMatchObject({
      text: "Couldn't start #142: T3 Code isn't open. Open it, then try again.",
      tone: 'warn',
      actions: [{ kind: 'retry' }],
    });
  });
});

describe('Start ready', () => {
  const r = (id: number, over: Partial<RowDto> = {}): RowDto => ({
    id, parent: null, depth: 0, position: id, title: `T${id}`, status: 'todo', origin: 'plan', blocked_reason: null,
    waiting: false, waits_on: [], has_children: false, progress: null, repos: [1], repos_from_subtasks: false,
    branch: null, ready: true, ready_since: null, ready_pill: false, quiet_days: null, needs: false,
    needs_stale: false, live: null, last_at: '2026-11-26T10:00:00.000Z', last_actor: 'plan', launched_in: null, ...over,
  });
  // 3 › phase 4 (steps 5, 6) · phase 7, waiting on 4 (step 8) · phase 22 (steps 23 doing, 24 waiting on 23)
  const list = [
    r(3, { has_children: true, ready: false, status: 'doing' }),
    r(4, { parent: 3, has_children: true, ready: false }),
    r(5, { parent: 4, repos: [] }),
    r(6, { parent: 4 }),
    r(7, { parent: 3, has_children: true, ready: false, waiting: true, waits_on: [4] }),
    r(8, { parent: 7 }),
    r(22, { parent: 3, has_children: true, ready: false }),
    r(23, { parent: 22, status: 'doing', ready: false }),
    r(24, { parent: 22, waiting: true, waits_on: [23], ready: false }),
  ];
  const rows = new Map(list.map((x) => [x.id, x]));
  const order = list.map((x) => x.id);

  it('picks ready leaves, skipping phases that wait and steps that run or wait', () => {
    expect(readySteps(rows, order, 3)).toEqual([5, 6]);
    expect(readySteps(rows, order, 4)).toEqual([5, 6]);
    expect(readySteps(rows, order, 7)).toEqual([8]);
    expect(readySteps(rows, order, 22)).toEqual([]);
  });

  it('says what started in one toast, and what needs a repo', () => {
    const started = { task: 6, report: report({ started: [thread(1)] }) };
    expect(startReadyToast(4, { started: [started], noRepo: [5], failed: [] })).toEqual({
      text: 'Started step of #4: #6. #5 has no repo: open it and press Do it to choose where.',
      tone: 'warn',
    });
    expect(startReadyToast(3, { started: [started, { ...started, task: 23 }], noRepo: [], failed: [] }).text).toBe('Started 2 steps of #3: #6 and #23.');
    expect(startReadyToast(3, { started: [], noRepo: [], failed: [{ task: 6, message: 'T3 is not running.' }] }).text).toBe('#6 failed: T3 is not running.');
  });
});
