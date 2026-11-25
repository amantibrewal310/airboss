import { describe, expect, it } from 'vitest';
import {
  isTreeLayout,
  parentsList,
  sidebarCounts,
  viewCount,
  viewTitle,
  visibleItems,
  type VisibleItem,
} from '../../src/lib/logic/views';
import { buildRows } from '../../src/lib/mock/viewmodel';
import { stateFromSeed } from '../../src/lib/mock/model';
import { loadFixture } from '../../src/lib/mock/fixtures';

const { rows, order } = buildRows(stateFromSeed(loadFixture('full')));
const ids = (items: VisibleItem[]) => items.flatMap((i) => (i.kind === 'row' ? [i.id] : []));
const none = new Set<number>();

describe('views (SPEC-app "List views")', () => {
  it('All tasks (Tree) is the whole tree minus dropped, in tree order with depth', () => {
    const items = visibleItems(rows, order, 'all', none, 'tree');
    expect(ids(items).slice(0, 7)).toEqual([142, 143, 144, 145, 146, 147, 200]);
    expect(ids(items)).not.toContain(172); // dropped subtask
    expect(ids(items)).not.toContain(176); // dropped top-level task
    expect(ids(items)).toContain(98); // done tasks stay, dimmed
    const r144 = items.find((i) => i.kind === 'row' && i.id === 144)!;
    expect(r144).toMatchObject({ depth: 1, pos: 2, size: 5 });
  });

  it('hides a folded parent’s subtree but keeps the parent', () => {
    const items = ids(visibleItems(rows, order, 'all', new Set([142, 160]), 'tree'));
    expect(items).toContain(142);
    expect(items).not.toContain(143);
    expect(items).toContain(160);
    expect(items).not.toContain(163);
  });

  it('By status groups in order: In progress, In review, Blocked, Todo, Waiting, Proposed, Done', () => {
    const items = visibleItems(rows, order, 'all', none, 'status');
    const groups = items.filter((i) => i.kind === 'group').map((i) => (i.kind === 'group' ? i.label : ''));
    expect(groups).toEqual(['In progress', 'In review', 'Blocked', 'Todo', 'Waiting', 'Proposed', 'Done']);
    const waiting = items.findIndex((i) => i.kind === 'group' && i.label === 'Waiting');
    expect(items[waiting + 1]).toMatchObject({ kind: 'row', id: 145, depth: 0 });
  });

  it('Ready to start lists ready leaves, flat; In progress lists doing and in review', () => {
    expect(ids(visibleItems(rows, order, 'ready', none, 'tree'))).toEqual(
      [...order].filter((id) => rows.get(id)!.ready),
    );
    for (const id of ids(visibleItems(rows, order, 'progress', none, 'tree'))) {
      expect(['doing', 'in_review']).toContain(rows.get(id)!.status);
    }
    expect(visibleItems(rows, order, 'ready', none, 'tree').every((i) => i.kind === 'row' && i.depth === 0)).toBe(true);
  });

  it('a repo’s view holds tasks whose own or inherited repos include it', () => {
    const web = ids(visibleItems(rows, order, 'repo:1', none, 'tree'));
    expect(web).toContain(142); // a parent, through its subtasks
    expect(web).toContain(144);
    expect(web).not.toContain(143);
  });

  it('a parent’s view is its subtree, its children at depth 0', () => {
    const items = visibleItems(rows, order, 'parent:160', none, 'tree');
    expect(items[0]).toMatchObject({ id: 161, depth: 0 });
    expect(items.find((i) => i.kind === 'row' && i.id === 163)).toMatchObject({ depth: 1 });
    expect(isTreeLayout('parent:160', 'status')).toBe(true);
    expect(isTreeLayout('ready', 'tree')).toBe(false);
  });

  it('counts the sidebar: ready leaves, plans, in progress, open tasks', () => {
    const c = sidebarCounts(rows);
    expect(c.ready).toBe([...rows.values()].filter((r) => r.ready).length);
    expect(c.plans).toBe(5); // Booking flow, Refunds, Dark mode tokens, Menu-bar app v1, Accessibility audit
    expect(parentsList(rows, order).map((r) => r.id)).toEqual([142, 200, 110, 160, 190]);
    expect(viewCount(rows, order, 'parent:142')).toBe(4);
    expect(viewTitle('repo:2', rows, [{ id: 2, name: 'booking-api', path: '', remote_url: null }])).toBe('booking-api');
  });
});
