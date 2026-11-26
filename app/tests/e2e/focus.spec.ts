import { expect, test, type Page } from '@playwright/test';

/**
 * Focus: traps that hold with macOS Keyboard navigation off (WebKit's Tab then skips buttons),
 * Esc that always closes an overlay, focus that never lands on <body>, and no browser context
 * menu outside text fields.
 */

const activeIn = (page: Page, selector: string) => page.evaluate((s) => !!document.activeElement?.closest(s), selector);
const onBody = (page: Page) => page.evaluate(() => document.activeElement === document.body || document.activeElement === null);

test('Settings keeps Tab inside and Esc closes it', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=144');
  await page.keyboard.press('Meta+Comma');
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog).toBeVisible();
  const seen = new Set<string>();
  for (let i = 0; i < 10; i++) {
    await page.keyboard.press('Tab');
    expect(await activeIn(page, '[role="dialog"]')).toBe(true);
    seen.add(await page.evaluate(() => document.activeElement?.textContent?.trim().slice(0, 20) ?? ''));
  }
  // Buttons are reachable, not only fields.
  expect([...seen].some((t) => /Done|Install|Reinstall|Copy agent note|Add repos|Edit|Remove/.test(t))).toBe(true);
  await page.keyboard.press('Shift+Tab');
  expect(await activeIn(page, '[role="dialog"]')).toBe(true);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  expect(await onBody(page)).toBe(false);
});

test('the ⌘L popover keeps Tab inside and Esc closes it', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=144');
  await page.getByRole('tree').focus();
  await page.keyboard.press('Meta+l');
  const pop = page.getByRole('dialog', { name: /Add a link/ });
  await expect(pop).toBeVisible();
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press('Tab');
    expect(await activeIn(page, '[role="dialog"]')).toBe(true);
  }
  await page.keyboard.press('Escape');
  await expect(pop).toHaveCount(0);
});

test('Esc closes an overlay even when focus has escaped it', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=144');
  await page.keyboard.press('Meta+Comma');
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Settings' })).toHaveCount(0);
});

test('first run reaches its buttons by Tab', async ({ page }) => {
  await page.goto('/?fixture=empty');
  const reached = new Set<string>();
  for (let i = 0; i < 25; i++) {
    await page.keyboard.press('Tab');
    reached.add(await page.evaluate(() => (document.activeElement?.tagName === 'BUTTON' ? document.activeElement.textContent?.trim() : '') ?? ''));
  }
  expect([...reached].some((t) => t.startsWith('New task'))).toBe(true);
  expect([...reached].some((t) => t.startsWith('Change hotkey'))).toBe(true);
});

test('focus starts on the inbox and comes back to the list after a rename', async ({ page }) => {
  await page.goto('/?fixture=design');
  await page.waitForTimeout(200);
  expect(await activeIn(page, '[role="listbox"][aria-label="Needs me"]')).toBe(true);
  await page.keyboard.press('g');
  await page.keyboard.press('a');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await page.keyboard.press('Escape');
  expect(await activeIn(page, '[role="tree"]')).toBe(true);
  await page.keyboard.press('Enter');
  await page.keyboard.type(' (renamed)');
  await page.keyboard.press('Enter');
  expect(await activeIn(page, '[role="tree"]')).toBe(true);
});

test('focus stays in the panel after editing notes', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=152');
  await page.waitForTimeout(200);
  await page.locator('aside.peek .doc').click();
  await page.locator('aside.peek textarea[aria-label="Notes"]').waitFor();
  await page.keyboard.type('Read the README first.');
  await page.keyboard.press('Meta+Enter');
  expect(await onBody(page)).toBe(false);
  expect(await activeIn(page, '[data-zone="panel"]')).toBe(true);
  await page.locator('aside.peek .doc').click();
  await page.locator('aside.peek textarea[aria-label="Notes"]').waitFor();
  await page.keyboard.press('Escape');
  expect(await onBody(page)).toBe(false);
});

test('right-click shows the app’s menus only; text fields keep the browser’s', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=144');
  const prevented = (selector: string) =>
    page.locator(selector).first().evaluate((el) => {
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      el.dispatchEvent(e);
      return e.defaultPrevented;
    });
  expect(await prevented('header.head')).toBe(true);
  expect(await prevented('aside[aria-label="Sidebar"]')).toBe(true);
  expect(await prevented('aside.peek h1')).toBe(true);
  expect(await prevented('textarea[aria-label="Leave a note for the agent"]')).toBe(false);
});

test('notes and activity text can be selected and copied', async ({ page }) => {
  await page.goto('/?fixture=design&view=all&select=144');
  await page.waitForTimeout(200);
  const userSelect = (selector: string) => page.locator(selector).first().evaluate((el) => getComputedStyle(el).webkitUserSelect || getComputedStyle(el).userSelect);
  expect(await userSelect('aside.peek .doc')).toBe('text');
  expect(await userSelect('aside.peek .ev .tx')).toBe('text');
  // Selecting text in the notes doesn't open the editor.
  await page.locator('aside.peek .doc').dblclick();
  await page.waitForTimeout(300);
  expect(await page.locator('aside.peek textarea[aria-label="Notes"]').count()).toBe(0);
});
