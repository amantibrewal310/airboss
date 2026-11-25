/*
  Selection rules (SPEC-app "UI state"), pure so each rule has a test.
*/

import type { RowDto, TaskId } from '../types';
import { ancestorsOf, isListView, viewHolds, type View } from './views';

/**
 * When a patch, a view change or a fold removes the selected row from the visible list, the
 * row now at the same index is selected, clamped to the last row. An empty list selects
 * nothing (and the panel closes).
 */
export function keepSelection<K>(selected: K | null, prevIndex: number, next: K[]): { selected: K | null; index: number } {
  if (selected !== null) {
    const i = next.indexOf(selected);
    if (i !== -1) return { selected, index: i };
  }
  if (!next.length) return { selected: null, index: -1 };
  if (selected === null) return { selected: null, index: -1 };
  const index = Math.min(Math.max(prevIndex, 0), next.length - 1);
  return { selected: next[index]!, index };
}

/** Folding a parent that holds the selection selects that parent. */
export function selectionAfterFold(rows: Map<TaskId, RowDto>, selected: TaskId | null, folded: TaskId): TaskId | null {
  if (selected === null) return null;
  return ancestorsOf(rows, selected).includes(folded) ? folded : selected;
}

/**
 * Revealing a task (Jump, menu bar, toasts) unfolds its ancestors and switches to All tasks
 * if the current view doesn't show it.
 */
export function reveal(
  rows: Map<TaskId, RowDto>,
  order: TaskId[],
  view: View,
  id: TaskId,
): { view: View; unfold: TaskId[] } {
  const unfold = ancestorsOf(rows, id);
  const stays = isListView(view) && viewHolds(rows, order, view, id);
  return { view: stays ? view : 'all', unfold };
}

/** A map selection clears when its station disappears. */
export function keepMapSelection(selected: TaskId | null, nodes: TaskId[]): TaskId | null {
  return selected !== null && nodes.includes(selected) ? selected : null;
}

/** Hands out request numbers and says whether a response is still the latest. */
export class Requests {
  private n = 0;
  next(): number {
    return ++this.n;
  }
  isLatest(req: number): boolean {
    return req === this.n;
  }
}
