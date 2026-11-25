import { describe, expect, it } from 'vitest';
import { indexAt, prefixSums, scrollToShow, visibleRange } from '../../src/lib/logic/virtual';

describe('virtual list maths', () => {
  const rows = (n: number) => prefixSums(new Array(n).fill(38), 6);

  it('builds prefix sums from fixed heights, with mixed group headers', () => {
    expect([...prefixSums([34, 38, 38], 6)]).toEqual([6, 40, 78, 116]);
  });

  it('finds the first visible item by binary search', () => {
    const o = rows(2000);
    expect(indexAt(o, 0)).toBe(0);
    expect(indexAt(o, 6 + 38 * 10)).toBe(10);
    expect(indexAt(o, 6 + 38 * 10 - 1)).toBe(9);
    expect(indexAt(o, 1e9)).toBe(1999);
  });

  it('renders the viewport plus 8 rows of overscan, well under 60 DOM rows', () => {
    const o = rows(2000);
    const r = visibleRange(o, 38 * 500, 812, 8);
    expect(r.end - r.start).toBeLessThan(60);
    expect(r.start).toBe(499 - 8);
    expect(visibleRange(o, 0, 812, 8).start).toBe(0);
    expect(visibleRange(prefixSums([]), 0, 812).end).toBe(0);
  });

  it('scrolls just enough to keep the selection visible (nearest)', () => {
    const o = rows(100);
    expect(scrollToShow(o, 5, 0, 400)).toBeNull();
    // Below the viewport: align its bottom with the viewport's bottom.
    expect(scrollToShow(o, 20, 0, 400)).toBe(6 + 38 * 21 - 400);
    // Above the viewport: align its top, below a 34px sticky header.
    expect(scrollToShow(o, 10, 1000, 400, 34)).toBe(6 + 380 - 34);
    // The first row scrolls all the way up, padding included.
    expect(scrollToShow(o, 0, 200, 400)).toBe(0);
  });
});
