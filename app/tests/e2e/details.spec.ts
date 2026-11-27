import { expect, test } from '@playwright/test';

/** P2 details: the sidebar footer, the highlight on the page, the light segmented control, the rail. */

test('the sidebar footer hairline is inset to the sidebar’s padding', async ({ page }) => {
  await page.goto('/?fixture=design');
  const box = await page.locator('.conn').evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, border: getComputedStyle(el).borderTopWidth };
  });
  expect(box.border).toBe('1px');
  expect(box.left).toBe(8);
  expect(box.right).toBeGreaterThanOrEqual(222);
  expect(box.right).toBeLessThanOrEqual(224);
});

test('the full page keeps the sidebar highlight on its view', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=200');
  await expect(page.locator('nav .nav.on')).toContainText('All tasks');
  await page.keyboard.press('Meta+ArrowDown');
  await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
  await expect(page.locator('nav .nav.on')).toContainText('All tasks');
});

test('in light mode the selected segment is raised white', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/?fixture=design&view=all');
  const bg = await page.locator('.seg .on').first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(bg).toBe('rgb(255, 255, 255)');
});

test('a parent whose repos come from its subtasks offers no "+ Link repo"', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=200');
  await expect(page.locator('aside.peek')).toContainText('from subtasks');
  await expect(page.locator('aside.peek').getByText(/\+ Link (a )?repo/)).toHaveCount(0);
  await page.keyboard.press('Meta+ArrowDown');
  await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
  await expect(page.getByText(/\+ Link (a )?repo/)).toHaveCount(0);
  // A task with its own repo still offers it.
  await page.goto('/?fixture=design&view=all&select=144');
  await expect(page.locator('aside.peek').getByText('+ Link repo')).toHaveCount(1);
});

test('a parent’s panel reads as decisions: summary, Ready now, In progress, Next, Done folded', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=200');
  const panel = page.locator('aside.peek');
  await expect(panel).toContainText('1 of 8 done · 1 ready · 1 to review · 1 blocked');
  const section = (name: string) => panel.getByRole('region', { name: new RegExp(`^${name}`) });
  // Ready now holds only what can start; finished steps never sit above it.
  await expect(section('Ready now').locator('.sub')).toHaveText([/206/]);
  await expect(section('Ready now').getByRole('button', { name: 'Start ready' })).toBeVisible();
  await expect(section('In progress').locator('.sub .id')).toHaveText(['204', '202', '203']);
  await expect(section('Next').locator('.sub .id')).toHaveText(['205', '207', '208']);
  // Done is folded until asked.
  const done = panel.getByRole('button', { name: /^Done/ });
  await expect(done).toHaveAttribute('aria-expanded', 'false');
  await expect(panel.locator('.sub .id', { hasText: '201' })).toHaveCount(0);
  await done.click();
  await expect(panel.locator('.sub .id', { hasText: '201' })).toHaveCount(1);
  // No Status or Branch rows, and no "wave" anywhere.
  await expect(panel.locator('.props .l')).not.toContainText(['Status', 'Branch', 'Links']);
  await expect(panel).not.toContainText(/wave/i);
});

test('a parent’s proposals show one by one, with Accept all for one agent turn', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=150');
  const needs = page.locator('aside.peek').getByRole('region', { name: /^Needs you/ });
  await expect(needs).toContainText('3 proposed together');
  await expect(needs.getByRole('button', { name: 'Accept', exact: true })).toHaveCount(3);
  await expect(needs.getByRole('button', { name: 'Reject' })).toHaveCount(3);
  await needs.locator('.prop', { hasText: '154' }).getByRole('button', { name: 'Accept', exact: true }).click();
  await expect(page.getByRole('status').last()).toContainText('Accepted #154');
  await expect(needs.getByRole('button', { name: 'Accept', exact: true })).toHaveCount(2);
  await needs.getByRole('button', { name: 'Accept all' }).click();
  await expect(needs).toHaveCount(0);
});
