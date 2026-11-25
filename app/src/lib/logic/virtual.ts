/*
  The virtual list's maths: fixed heights, a prefix-sum array rebuilt when the visible list
  changes, and binary search for the first visible item.
*/

/** offsets[i] is where item i starts (after `padTop`); offsets[n] is where the items end. */
export function prefixSums(heights: ArrayLike<number>, padTop = 0): Float64Array {
  const out = new Float64Array(heights.length + 1);
  out[0] = padTop;
  for (let i = 0; i < heights.length; i++) out[i + 1] = out[i]! + heights[i]!;
  return out;
}

/** The item covering y (clamped to the list). */
export function indexAt(offsets: Float64Array, y: number): number {
  const n = offsets.length - 1;
  if (n <= 0) return 0;
  let lo = 0;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (offsets[mid]! <= y) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Items to render: those in the viewport plus `overscan` on each side. `end` is exclusive. */
export function visibleRange(
  offsets: Float64Array,
  scrollTop: number,
  viewport: number,
  overscan = 8,
): { start: number; end: number } {
  const n = offsets.length - 1;
  if (n <= 0) return { start: 0, end: 0 };
  const first = indexAt(offsets, scrollTop);
  const last = indexAt(offsets, scrollTop + Math.max(0, viewport - 1));
  return { start: Math.max(0, first - overscan), end: Math.min(n, last + 1 + overscan) };
}

/**
 * Scrolls just enough to show item `index` ("nearest", no smoothing). `coverTop` is the height
 * of anything drawn over the top of the viewport (the sticky group header). Returns the new
 * scrollTop, or null when the item is already visible.
 */
export function scrollToShow(
  offsets: Float64Array,
  index: number,
  scrollTop: number,
  viewport: number,
  coverTop = 0,
  padBottom = 0,
): number | null {
  if (index < 0 || index >= offsets.length - 1) return null;
  const top = offsets[index]!;
  const bottom = offsets[index + 1]!;
  if (top - coverTop < scrollTop) return Math.max(0, top - coverTop - (index === 0 ? offsets[0]! : 0));
  if (bottom > scrollTop + viewport) return bottom - viewport + (index === offsets.length - 2 ? padBottom : 0);
  return null;
}
