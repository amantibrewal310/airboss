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
