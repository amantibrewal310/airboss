import { expect, test, type Page } from '@playwright/test';

/**
 * Every main screen on Rust's recorded answers (`?fixture=recorded`, from
 * src/lib/mock/fixtures/recorded.json): no console errors, nothing unscripted, and axe clean in
 * both themes. The recording has one task_detail (142) and one run_map (200), so those are the
 * tasks opened here.
 */

const AXE = 'node_modules/axe-core/axe.min.js';

const SCREENS: [string, string, ((p: Page) => Promise<unknown>)?][] = [
  ['needs', '/?fixture=recorded'],
  ['all + panel', '/?fixture=recorded&view=all&select=142'],
  ['ready', '/?fixture=recorded&view=ready'],
  ['in progress', '/?fixture=recorded&view=progress'],
  ['plans', '/?fixture=recorded&view=plans'],
  ['page + run map', '/?fixture=recorded&view=all&select=200', (p) => p.keyboard.press('Meta+ArrowDown')],
  ['new task', '/?fixture=recorded&view=all&select=142', (p) => p.keyboard.press('c')],
  ['settings', '/?fixture=recorded&view=all&select=142', (p) => p.keyboard.press('Meta+Comma')],
];

for (const scheme of ['dark', 'light'] as const) {
  for (const [name, url, steps] of SCREENS) {
    test(`${name} (${scheme})`, async ({ page }) => {
      const problems: string[] = [];
      page.on('console', (m) => m.type() === 'error' && problems.push(m.text()));
      page.on('pageerror', (e) => problems.push(e.message));
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      await page.goto(url);
      await page.locator('#app > *').first().waitFor();
      await page.waitForTimeout(300);
      if (steps) await steps(page);
      await page.waitForTimeout(300);
      await expect(page.locator('#app')).not.toContainText('Something went wrong');
      await page.addScriptTag({ path: AXE });
      const violations = await page.evaluate(async () => {
        const axe = (window as unknown as { axe: { run: (c: Document, o: object) => Promise<{ violations: { id: string; impact: string; nodes: { target: string[] }[] }[] }> } }).axe;
        const r = await axe.run(document, { resultTypes: ['violations'] });
        return r.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical').map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
      });
      expect(problems).toEqual([]);
      expect(violations, violations.join('\n')).toEqual([]);
    });
  }
}

test('the panel and page show the recorded task, its activity in plain words', async ({ page }) => {
  await page.goto('/?fixture=recorded&view=all&select=142');
  const panel = page.locator('aside.peek');
  await expect(panel.getByRole('heading', { name: 'Booking flow' })).toBeVisible();
  // core's "<kind> <ref>" bodies read as the link's title, not the raw kind.
  await expect(panel).not.toContainText('slack /archives');
  await expect(panel).not.toContainText('doc ~/plans');
  await expect(panel).not.toContainText('waits on #');
});
