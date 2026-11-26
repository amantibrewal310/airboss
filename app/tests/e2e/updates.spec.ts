import { expect, test, type Page } from '@playwright/test';

/**
 * SPEC-updates on the mock build: the sidebar's card from an available update to Restarting…,
 * how it sits in the sidebar, and ⌘K's Check for updates.
 */

const card = (page: Page) => page.locator('[data-update]');
const title = (page: Page) => page.locator('#update-title');

test('available → Update and restart → the download’s progress → Installing… → Restarting…', async ({ page }) => {
  const problems: string[] = [];
  page.on('console', (m) => m.type() === 'error' && problems.push(m.text()));
  page.on('pageerror', (e) => problems.push(e.message));
  await page.goto('/?fixture=update');
  await expect(title(page)).toHaveText('Airboss 0.1.0-alpha.5 is available');
  // Every title and progress value the card shows, in order (some last only a moment).
  await page.evaluate(() => {
    const w = window as unknown as { __seen: { titles: string[]; values: string[] } };
    const seen = (w.__seen = { titles: [document.querySelector('#update-title')!.textContent!], values: [] });
    const note = (list: string[], x: string | null | undefined) => x && list.at(-1) !== x && list.push(x);
    new MutationObserver(() => {
      note(seen.titles, document.querySelector('#update-title')?.textContent);
      note(seen.values, document.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow'));
    }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
  });
  await page.getByRole('button', { name: 'Update and restart' }).click();

  const bar = page.getByRole('progressbar', { name: 'Downloading Airboss 0.1.0-alpha.5' });
  await expect(title(page)).toHaveText('Downloading Airboss 0.1.0-alpha.5…');
  await expect(bar).toHaveAttribute('aria-valuemax', '100');
  await expect(title(page)).toHaveText('Restarting…');
  await expect(card(page).getByRole('button')).toHaveCount(0);
  const seen = await page.evaluate(() => (window as unknown as { __seen: { titles: string[]; values: string[] } }).__seen);
  expect(seen.titles).toEqual(['Airboss 0.1.0-alpha.5 is available', 'Downloading Airboss 0.1.0-alpha.5…', 'Installing Airboss 0.1.0-alpha.5…', 'Restarting…']);
  // The percent moved on while it downloaded.
  expect(seen.values).toEqual(['0', '25', '50', '75', '100']);
  expect(problems).toEqual([]);
});

test('the card sits above the connections line, inside the sidebar, and never moves the views', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 760 });
  await page.goto('/?fixture=design');
  const nav = page.locator('nav[aria-label="Views"] [data-nav]').first();
  await nav.waitFor();
  const before = await nav.boundingBox();
  await page.evaluate(() =>
    (window as unknown as { __airbossMock: { emit: (e: string, p: unknown) => void } }).__airbossMock.emit('update:changed', {
      state: 'failed',
      version: '0.1.0-alpha.5',
      message: 'The download’s signature didn’t match, so it wasn’t installed.',
    }),
  );
  await expect(card(page)).toBeVisible();
  expect(await nav.boundingBox()).toEqual(before);
  const side = page.locator('aside[data-zone="sidebar"]');
  const fits = await side.evaluate((el) => {
    const c = el.querySelector('[data-update]')!.getBoundingClientRect();
    const s = el.getBoundingClientRect();
    const conn = el.querySelector('.conn')!.getBoundingClientRect();
    return { noScroll: el.scrollWidth <= el.clientWidth, inside: c.left >= s.left && c.right <= s.right, above: c.bottom <= conn.top };
  });
  expect(fits).toEqual({ noScroll: true, inside: true, above: true });
});

test('Tab reaches the card’s buttons in the sidebar’s order, before the connections line', async ({ page }) => {
  await page.goto('/?fixture=update');
  await expect(card(page)).toBeVisible();
  await page.getByRole('button', { name: 'Update and restart' }).focus();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: "What's new" })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Later' })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.locator('.conn button')).toBeFocused();
  // Later from the keyboard: the card goes and focus stays in the sidebar.
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Enter');
  await expect(card(page)).toHaveCount(0);
  await expect(page.locator('.conn button')).toBeFocused();
});

test('⌘K › Check for updates finds the release and says so', async ({ page }) => {
  await page.goto('/?fixture=update');
  await page.getByRole('button', { name: 'Later' }).click();
  await expect(card(page)).toHaveCount(0);
  await page.keyboard.press('Meta+k');
  await page.keyboard.type('check for updates');
  await page.getByRole('option', { name: 'Check for updates' }).click();
  await expect(page.locator('[data-toast]').filter({ hasText: 'Airboss 0.1.0-alpha.5 is available.' })).toBeVisible();
  await expect(title(page)).toHaveText('Airboss 0.1.0-alpha.5 is available');
});
