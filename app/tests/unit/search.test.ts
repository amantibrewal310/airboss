import { describe, expect, it } from 'vitest';
import { invalidTarget, score, searchCommands, searchTasks } from '../../src/lib/logic/search';
import { buildRows } from '../../src/lib/mock/viewmodel';
import { stateFromSeed } from '../../src/lib/mock/model';
import { loadFixture } from '../../src/lib/mock/fixtures';

const { rows, order } = buildRows(stateFromSeed(loadFixture('full')));

describe('search (⌘K Jump)', () => {
  it('ranks substrings above scattered letters, and word starts first', () => {
    expect(score('ref', 'Refunds')).toBeGreaterThan(score('ref', 'Payments refactor stack'));
    expect(score('rfd', 'Refunds')).toBeGreaterThan(0);
    expect(score('xyz', 'Refunds')).toBe(0);
  });

  it('matches “ref” like the design: Refunds first', () => {
    const ids = searchTasks(rows, order, 'ref').map((r) => r.id);
    expect(ids[0]).toBe(200);
    expect(ids).toEqual(expect.arrayContaining([202, 204, 205]));
    expect(ids.length).toBeLessThanOrEqual(8);
  });

  it('puts a typed number’s task first', () => {
    expect(searchTasks(rows, order, '146')[0]!.id).toBe(146);
  });

  it('shows the 5 most recently changed open tasks with no query, dropped never', () => {
    const recent = searchTasks(rows, order, '');
    expect(recent).toHaveLength(5);
    expect(recent.every((r) => r.status !== 'done' && r.status !== 'dropped')).toBe(true);
    expect(searchTasks(rows, order, 'Brewfile')).toHaveLength(0);
  });

  it('offers 4 commands with no query and 5 at most with one', () => {
    const cmds = ['Go to Plans', 'Go to Ready to start', 'New task', 'Do it', 'Plan it', 'Go to All tasks'].map((label) => ({ label }));
    expect(searchCommands(cmds, '')).toHaveLength(4);
    expect(searchCommands(cmds, 'go').length).toBeLessThanOrEqual(5);
    expect(searchCommands(cmds, 'plan')[0]!.label).toBe('Plan it');
  });

  it('greys out targets the picker can’t use, with the reason', () => {
    expect(invalidTarget(rows, 'waits_on', 144, 144)).toMatch(/itself/);
    expect(invalidTarget(rows, 'waits_on', 144, 142)).toBe('Its own parent');
    expect(invalidTarget(rows, 'waits_on', 142, 144)).toBe('Its own subtask');
    expect(invalidTarget(rows, 'waits_on', 144, 200)).toBeNull();
    expect(invalidTarget(rows, 'move_under', 142, 143)).toBe('One of its subtasks');
    expect(invalidTarget(rows, 'move_under', 144, 98)).toMatch(/done/);
    expect(invalidTarget(rows, 'move_under', 144, 200)).toBeNull();
  });
});
