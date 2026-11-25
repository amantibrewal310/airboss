import { expect, test, type Page } from '@playwright/test';

/**
 * axe-core on every screen, in dark and light, with and without Increase Contrast. Serious and
 * critical violations fail (colour contrast, names, nested controls); moderate ones are reported.
 */

/** Relative to app/, where Playwright runs. */
const AXE = 'node_modules/axe-core/axe.min.js';

type Screen = [name: string, url: string, steps?: (page: Page) => Promise<void>];

const SCREENS: Screen[] = [
  ['needs', '/?fixture=design'],
  ['needs-full', '/?fixture=full'],
  ['all+panel', '/?fixture=design&view=all&select=144'],
  ['full-all', '/?fixture=full&view=all&select=180'],
  ['plans', '/?fixture=design&view=plans'],
  ['page', '/?fixture=design&view=all&select=200', (p) => p.keyboard.press('Meta+ArrowDown')],
  ['jump', '/?fixture=design&view=all&select=144', async (p) => (await p.keyboard.press('Meta+k'), p.keyboard.type('ref'))],
  ['new-task', '/?fixture=design&view=all&select=144', (p) => p.keyboard.press('c')],
  ['settings', '/?fixture=design&view=all&select=144', (p) => p.keyboard.press('Meta+Comma')],
  ['sheet', '/?fixture=design&view=all&select=144', (p) => p.keyboard.press('?')],
  ['status-menu', '/?fixture=design&view=all&select=144', async (p) => (await p.getByRole('tree').focus(), p.keyboard.press('s'))],
  ['first-run', '/?fixture=empty'],
  ['welcome', '/?fixture=welcome'],
  ['nothing', '/?fixture=nothing'],
  ['t3missing', '/?fixture=t3missing&view=all&select=144'],
  ['dberror', '/?fixture=dberror'],
];

const MODES = [
  { name: 'dark', colorScheme: 'dark', contrast: 'no-preference' },
  { name: 'light', colorScheme: 'light', contrast: 'no-preference' },
  { name: 'dark, Increase Contrast', colorScheme: 'dark', contrast: 'more' },
  { name: 'light, Increase Contrast', colorScheme: 'light', contrast: 'more' },
] as const;

for (const mode of MODES) {
  test.describe(mode.name, () => {
    for (const [name, url, steps] of SCREENS) {
      test(name, async ({ page }) => {
        await page.emulateMedia({ colorScheme: mode.colorScheme, contrast: mode.contrast, reducedMotion: 'reduce' });
        await page.goto(url);
        await page.locator('#app > *').first().waitFor();
        await page.waitForTimeout(300);
        if (steps) await steps(page);
        await page.waitForTimeout(300);
        await page.addScriptTag({ path: AXE });
        const violations = await page.evaluate(async () => {
          const axe = (window as unknown as { axe: { run: (c: Document, o: object) => Promise<{ violations: { id: string; impact: string; nodes: { target: string[]; failureSummary: string }[] }[] }> } }).axe;
          const r = await axe.run(document, { resultTypes: ['violations'] });
          return r.violations
            .filter((v) => v.impact === 'serious' || v.impact === 'critical')
            .map((v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => `${n.target.join(' ')} — ${n.failureSummary.split('\n').slice(1, 2).join(' ')}`).join(' | ')}`);
        });
        expect(violations, violations.join('\n')).toEqual([]);
      });
    }
  });
}
