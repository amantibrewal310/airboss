import { expect, test } from '@playwright/test';

/**
 * SPEC-app "Performance": 2,000 rows at 60 fps. Scrolls the whole list of the perf fixture
 * (2,000 tasks, every parent open) in 3 s and records each animation frame's interval.
 * Pass: 95th percentile under 16.7 ms (plus vsync jitter) and no frame over 50 ms.
 */
test('2,000 rows scroll at 60 fps', async ({ page, browserName }, info) => {
  await page.goto('/?fixture=perf&view=all');
  const list = page.getByRole('tree');
  await expect(list).toBeVisible();
  const result = await page.evaluate(async () => {
    const el = document.querySelector<HTMLElement>('[role="tree"]')!;
    const rows = el.querySelectorAll('[data-row]').length;
    const total = el.scrollHeight - el.clientHeight;
    const intervals: number[] = [];
    let maxRows = 0;
    // Let the page settle for a few frames first.
    await new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
    await new Promise<void>((resolve) => {
      const start = performance.now();
      let last = start;
      const step = (t: number) => {
        if (t !== start) intervals.push(t - last);
        last = t;
        const p = Math.min(1, (t - start) / 3000);
        el.scrollTop = p * total;
        maxRows = Math.max(maxRows, el.querySelectorAll('[data-row]').length);
        if (p < 1) requestAnimationFrame(step);
        else resolve();
      };
      requestAnimationFrame(step);
    });
    const sorted = [...intervals].sort((a, b) => a - b);
    const pct = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]!;
    return {
      frames: intervals.length,
      p50: pct(0.5),
      p95: pct(0.95),
      max: sorted[sorted.length - 1]!,
      over20: intervals.filter((d) => d > 20).length,
      scrollHeight: el.scrollHeight,
      domRowsAtStart: rows,
      maxDomRows: maxRows,
      reachedBottom: Math.abs(el.scrollTop - total) < 2,
    };
  });
  const line = `${browserName}: ${result.frames} frames in 3 s; p50 ${result.p50.toFixed(2)} ms, p95 ${result.p95.toFixed(2)} ms, max ${result.max.toFixed(2)} ms, ${result.over20} over 20 ms; DOM rows ≤ ${result.maxDomRows}; list ${result.scrollHeight}px`;
  console.log(line);
  info.annotations.push({ type: 'frames', description: line });
  expect(result.reachedBottom).toBe(true);
  expect(result.maxDomRows).toBeLessThan(60);
  expect(result.p95).toBeLessThan(17.5);
  expect(result.max).toBeLessThan(50);
});
