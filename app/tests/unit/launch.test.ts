import { describe, expect, it } from 'vitest';
import { launchToast } from '../../src/lib/logic/launch';
import type { LaunchReport, Repo, StartedThread } from '../../src/lib/types';

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
      text: 'Started a thread for 142 in booking-web',
      actions: [{ kind: 'open_thread' }],
    });
  });
  it('several, all started', () => {
    expect(launchToast(142, 'do_it', report({ started: [thread(1), thread(2)] }), repos)).toMatchObject({
      text: 'Started 2 threads for 142 on task-142-booking-flow',
      actions: [],
    });
  });
  it('research only', () => {
    expect(launchToast(142, 'do_it', report({ started: [thread(null)] }), repos).text).toBe('Started a research thread for 142');
  });
  it('Plan it', () => {
    expect(launchToast(142, 'plan_it', report({ started: [thread(1)] }), repos).text).toBe(
      'Started a planning thread for 142. Its steps become subtasks when you settle the plan.',
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
      text: '142 already has threads in booking-web and booking-api',
      actions: [{ kind: 'again' }],
    });
  });
  it('none started, failed: Retry', () => {
    expect(launchToast(142, 'do_it', report({ failed: [[null, { code: 't3_not_running', message: "T3 Code isn't open. Open it, then try again." }]] }), repos)).toMatchObject({
      text: "Couldn't start 142: T3 Code isn't open. Open it, then try again.",
      tone: 'warn',
      actions: [{ kind: 'retry' }],
    });
  });
});
