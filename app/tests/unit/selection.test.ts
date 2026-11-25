import { describe, expect, it } from 'vitest';
import { keepMapSelection, keepSelection, Requests, reveal, selectionAfterFold } from '../../src/lib/logic/selection';
import { buildRows } from '../../src/lib/mock/viewmodel';
import { stateFromSeed } from '../../src/lib/mock/model';
import { loadFixture } from '../../src/lib/mock/fixtures';

const { rows, order } = buildRows(stateFromSeed(loadFixture('full')));

describe('selection rules (SPEC-app "UI state")', () => {
  it('keeps a selection that is still visible, at its new index', () => {
    expect(keepSelection(3, 0, [1, 2, 3])).toEqual({ selected: 3, index: 2 });
  });

  it('selects the row now at the same index when the selected row leaves', () => {
    expect(keepSelection(2, 1, [1, 3, 4])).toEqual({ selected: 3, index: 1 });
  });

  it('clamps to the last row', () => {
    expect(keepSelection(9, 5, [1, 2])).toEqual({ selected: 2, index: 1 });
  });

  it('selects nothing when the list is empty', () => {
    expect(keepSelection(1, 0, [])).toEqual({ selected: null, index: -1 });
  });

  it('selects the parent when folding a parent that holds the selection', () => {
    expect(selectionAfterFold(rows, 163, 160)).toBe(160);
    expect(selectionAfterFold(rows, 163, 162)).toBe(162);
    expect(selectionAfterFold(rows, 144, 160)).toBe(144);
  });

  it('reveals a task by unfolding its ancestors, switching to All tasks when the view lacks it', () => {
    expect(reveal(rows, order, 'ready', 163)).toEqual({ view: 'all', unfold: [162, 160] });
    expect(reveal(rows, order, 'parent:160', 163)).toEqual({ view: 'parent:160', unfold: [162, 160] });
    expect(reveal(rows, order, 'needs', 144).view).toBe('all');
  });

  it('uses the same-index rule for Needs me items', () => {
    expect(keepSelection('b', 1, ['a', 'c'])).toEqual({ selected: 'c', index: 1 });
  });

  it('clears a map selection when its station disappears', () => {
    expect(keepMapSelection(202, [201, 202])).toBe(202);
    expect(keepMapSelection(202, [201])).toBeNull();
  });

  it('ignores stale responses by request number', () => {
    const r = new Requests();
    const a = r.next();
    const b = r.next();
    expect(r.isLatest(a)).toBe(false);
    expect(r.isLatest(b)).toBe(true);
  });
});
