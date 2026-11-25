import { expect, test, type Page } from '@playwright/test';

/**
 * Screenshot baselines for the twelve web screens in docs/design/screens/ (01–07, 09–13), in
 * dark and in light: 24 baselines at 1440 × 900 with animations off, on the `design` fixture
 * (the design's sample data). They were compared with the design PNGs by eye; after that they
 * guard against regressions. Screen 08 (the native menu-bar menu) is checked by hand (QA.md).
 */

type Screen = { name: string; url: string; steps?: (page: Page) => Promise<void> };

const paste = (selector: string, text: string) => (page: Page) =>
  page.locator(selector).evaluate((el, t) => {
    const dt = new DataTransfer();
    dt.setData('text/plain', t);
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  }, text);

const SCREENS: Screen[] = [
  { name: '01-all-tasks-detail', url: '/?fixture=design&view=all&select=144' },
  { name: '02-needs-me', url: '/?fixture=design' },
  {
    name: '03-task-page-run-order',
    url: '/?fixture=design&view=all&select=200',
    steps: async (page) => {
      await page.keyboard.press('Meta+ArrowDown');
      await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
    },
  },
  { name: '04-plans', url: '/?fixture=design&view=plans' },
  {
    name: '05-new-task',
    url: '/?fixture=design&view=all&select=144',
    steps: async (page) => {
      await page.keyboard.press('c');
      await page.keyboard.type('Fix double booking when two people pick the same slot');
      await paste('textarea[aria-label="Notes"]', 'https://acme.slack.com/archives/C04BOOKING/p1732012345')(page);
      await page.keyboard.press('Tab');
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('Enter');
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('Enter');
    },
  },
  {
    name: '06-jump',
    url: '/?fixture=design&view=all&select=144',
    steps: async (page) => {
      await page.keyboard.press('Meta+k');
      await page.keyboard.type('ref');
    },
  },
  {
    name: '07-status-and-context-menus',
    url: '/?fixture=design&view=all&select=144',
    steps: async (page) => {
      await page.locator('[data-row="146"]').click({ button: 'right', position: { x: 370, y: 20 } });
      await expect(page.getByRole('menu')).toBeVisible();
    },
  },
  {
    name: '09-shortcuts',
    url: '/?fixture=design&view=all&select=144',
    steps: async (page) => {
      await page.keyboard.press('?');
    },
  },
  { name: '10-first-run', url: '/?fixture=empty' },
  { name: '11-nothing-needs-you', url: '/?fixture=nothing' },
  { name: '12-t3-missing', url: '/?fixture=t3missing&view=all&select=144' },
  { name: '13-light-mode', url: '/?fixture=design&view=all&select=144' },
];

for (const theme of ['dark', 'light'] as const) {
  test.describe(theme, () => {
    test.use({ colorScheme: theme });
    for (const s of SCREENS) {
      test(s.name, async ({ page }) => {
        await page.goto(s.url);
        await expect(page.getByRole('complementary', { name: 'Sidebar' })).toBeVisible();
        await page.evaluate(() => document.fonts.ready);
        if (s.steps) await s.steps(page);
        // Let the detail load and the 150–200 ms entrances finish.
        await page.waitForTimeout(400);
        await expect(page).toHaveScreenshot(`${s.name}-${theme}.png`, { fullPage: false });
      });
    }
  });
}
