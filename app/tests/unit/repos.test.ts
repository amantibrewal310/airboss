import { describe, expect, it } from 'vitest';
import { removeQuestion, tasksLinkedTo } from '../../src/lib/logic/repos';
import type { RowDto } from '../../src/lib/types';

const row = (id: number, repos: number[], fromSubtasks = false): RowDto => ({
  id, parent: null, depth: 0, position: id, title: `T${id}`, status: 'todo', origin: 'me', blocked_reason: null,
  waiting: false, waits_on: [], has_children: fromSubtasks, progress: null, repos, repos_from_subtasks: fromSubtasks,
  branch: null, ready: false, ready_since: null, ready_pill: false, quiet_days: null, needs: false,
  needs_stale: false, live: null, last_at: '2026-11-25T10:00:00.000Z', last_actor: 'me',
});

describe('Settings › Repos › Remove', () => {
  it('counts the tasks linked to a repo themselves, not parents showing it from subtasks', () => {
    const rows = new Map([
      [1, row(1, [2])],
      [2, row(2, [2, 3])],
      [3, row(3, [2], true)],
      [4, row(4, [3])],
    ]);
    expect(tasksLinkedTo(rows, 2)).toBe(2);
    expect(tasksLinkedTo(rows, 3)).toBe(2);
    expect(tasksLinkedTo(rows, 9)).toBe(0);
  });
  it('asks with how many tasks lose the link', () => {
    expect(removeQuestion('booking-web', 12)).toBe('Remove booking-web? 12 tasks lose the link.');
    expect(removeQuestion('booking-web', 1)).toBe('Remove booking-web? 1 task loses the link.');
    expect(removeQuestion('booking-web', 0)).toBe('Remove booking-web? No task is linked to it.');
  });
});
