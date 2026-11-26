import { expect, test, type Page } from '@playwright/test';

/**
 * R5, Agent stopped (`?fixture=stopped`: 202 Refund service, its agent settled 20 minutes ago with
 * no PR): keyboard only, in WebKit. It sits right after Failed runs, ↩ opens the thread, Space
 * marks the task done with Undo, and the card is axe clean in both themes.
 */

const AXE = 'node_modules/axe-core/axe.min.js';

async function selectedNeed(page: Page): Promise<string> {
  return (await page.locator('[role="option"][aria-selected="true"]').first().textContent()) ?? '';
}

async function toStopped(page: Page) {
  await page.goto('/?fixture=stopped');
  await expect(page.getByRole('heading', { name: 'Needs me' })).toBeVisible();
  for (let i = 0; i < 12 && !/Agent stopped/.test(await selectedNeed(page)); i++) await page.keyboard.press('ArrowDown');
  expect(await selectedNeed(page)).toMatch(/Agent stopped: 202 Refund service/);
}

test('Agent stopped: right after Failed runs; ↩ opens the thread; Space marks it done, ⌘Z undoes', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' || /UnscriptedCall/.test(m.text())) errors.push(m.text());
  });
  await toStopped(page);
  const heads = await page.locator('.ib-h span:first-child').allTextContents();
  expect(heads.indexOf('Stopped')).toBe(heads.indexOf('Failed runs') + 1);
  const card = page.locator('.detail .big');
  await expect(card).toContainText('Agent stopped · 20m ago · 202 Refund service');
  await expect(card.locator('.q')).toHaveText('Agent stopped · no PR yet');
  await expect(card.locator('.acts button')).toHaveText(['Open thread ↩', 'Mark done Space', 'Set in review', 'Snooze 3 days', 'Dismiss ⌘⌫'].map((t) => new RegExp(t.replace(/ /g, '\\s*'))));

  await page.keyboard.press('Enter');
  // Opening the thread changes nothing, so the item comes back after its 2 s local hide.
  await expect(page.getByRole('option', { name: /Agent stopped: 202/ })).toHaveCount(0);
  await expect(page.getByRole('option', { name: /Agent stopped: 202/ })).toHaveCount(1, { timeout: 4000 });
  await page.getByRole('option', { name: /Agent stopped: 202/ }).click();
  await page.locator('#app [role="listbox"]').focus();

  const toast = page.locator('[data-toast]').last();
  await page.keyboard.press(' ');
  await expect(toast).toContainText('Marked 202 done');
  await expect(page.getByRole('option', { name: /Agent stopped: 202/ })).toHaveCount(0);
  await page.keyboard.press('Meta+z');
  await expect(toast).toContainText('Undid');
  expect(errors).toEqual([]);
});

for (const scheme of ['dark', 'light'] as const) {
  test(`the Agent stopped card is axe clean (${scheme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
    await toStopped(page);
    await page.addScriptTag({ path: AXE });
    const violations = await page.evaluate(async () => {
      const axe = (window as unknown as { axe: { run: (c: Document, o: object) => Promise<{ violations: { id: string; impact: string; nodes: { target: string[] }[] }[] }> } }).axe;
      const r = await axe.run(document, { resultTypes: ['violations'] });
      return r.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical').map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
    });
    expect(violations, violations.join('\n')).toEqual([]);
  });
}

// Its five actions wrap inside the card (the main one first) from the smallest window up.
for (const width of [900, 1100, 1280]) {
  test(`the Agent stopped actions stay inside the card at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 760 });
    await toStopped(page);
    const { card, actions } = await page.locator('.detail .big').evaluate((el) => {
      const box = (e: Element) => {
        const r = e.getBoundingClientRect();
        return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, label: e.textContent?.trim() ?? '' };
      };
      return { card: box(el), actions: [...el.querySelectorAll('.acts button')].map(box) };
    });
    expect(actions).toHaveLength(5);
    expect(actions[0]!.label).toMatch(/^Open thread/);
    for (const a of actions) {
      expect(a.left, a.label).toBeGreaterThanOrEqual(card.left);
      expect(a.right, a.label).toBeLessThanOrEqual(card.right);
      expect(a.bottom, a.label).toBeLessThanOrEqual(card.bottom);
    }
    // Nothing is clipped either: each button is as wide as its label.
    const clipped = await page.locator('.detail .big .acts button').evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.textContent));
    expect(clipped).toEqual([]);
  });
}
