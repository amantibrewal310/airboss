import { describe, expect, it } from 'vitest';
import { canMarkDone, GROUP_ORDER, grouped, headlineOf, KIND_ICON, KIND_LABEL, mainLabel, otherLabel } from '../../src/lib/logic/needs';
import { GROUP_ORDER as MOCK_ORDER } from '../../src/lib/mock/attention';
import { resolve } from '../../src/lib/logic/commands';
import type { NeedsItem } from '../../src/lib/types';

const item = (kind: NeedsItem['kind'], group: NeedsItem['group'], key: string): NeedsItem => ({
  key,
  kind,
  group,
  task: 1,
  task_title: 'T',
  since: '2026-11-25T10:00:00.000Z',
  headline: 'h',
  context: 'c',
  target: { task: 1 },
  proposed: [],
  stale: false,
});

/** R5: Agent stopped, a doing task whose agents all stopped with no PR. */
describe('Agent stopped', () => {
  it('has its label, icon and actions', () => {
    expect(KIND_LABEL.agent_stopped).toBe('Agent stopped');
    expect(KIND_ICON.agent_stopped).toBe('stopped');
    expect(mainLabel('agent_stopped')).toBe('Open thread');
    expect(otherLabel('agent_stopped')).toBe('Dismiss');
    expect(headlineOf({ kind: 'agent_stopped', headline: 'Agent stopped · no PR yet' })).toBe('Agent stopped · no PR yet');
  });

  it('groups right after Failed runs, as Rust orders it', () => {
    expect(GROUP_ORDER.slice(2, 6)).toEqual(['failed_runs', 'stopped', 'due', 'proposed']);
    expect(MOCK_ORDER).toEqual(GROUP_ORDER);
    const g = grouped([item('proposed', 'proposed', 'p'), item('agent_stopped', 'stopped', 's'), item('failed_run', 'failed_runs', 'f')]);
    expect(g.map((x) => x.label)).toEqual(['Failed runs', 'Stopped', 'Proposed']);
  });

  it('Space in Needs me is Mark done; elsewhere it stays the list’s toggle', () => {
    expect(resolve('space', 'list', 'needs', true)?.id).toBe('needs_done');
    expect(resolve('space', 'panel', 'needs', true)?.id).toBe('needs_done');
    expect(resolve('space', 'list', 'list', true)?.id).toBe('toggle_done');
  });
});

/** Task 59: Due, an open task whose due date has come. */
describe('Due', () => {
  it('has its label, icon and actions', () => {
    expect(KIND_LABEL.due).toBe('Due');
    expect(KIND_ICON.due).toBe('calendar');
    expect(mainLabel('due')).toBe('Open');
    expect(otherLabel('due')).toBe('Dismiss');
    expect(canMarkDone('due')).toBe(true);
  });

  it('groups after Stopped and before Proposed, as Rust orders it', () => {
    const g = grouped([item('proposed', 'proposed', 'p'), item('due', 'due', 'd'), item('agent_stopped', 'stopped', 's')]);
    expect(g.map((x) => x.label)).toEqual(['Stopped', 'Due', 'Proposed']);
  });
});
