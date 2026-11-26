import { expect, test, type Page } from '@playwright/test';

/**
 * SPEC-app "End-to-end smoke test": keyboard only, on the mock build in WebKit.
 * Fails on any page error or any call the mock backend doesn't know (UnscriptedCall).
 */

async function selectedNeed(page: Page): Promise<string> {
  return (await page.locator('[role="option"][aria-selected="true"]').first().textContent()) ?? '';
}

test('keyboard-only smoke test', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' || /UnscriptedCall/.test(m.text())) errors.push(m.text());
  });
  await page.goto('/?fixture=full');
  const toast = page.locator('[data-toast]').last();

  // 1. Needs me: accept a proposal with ↩, then dismiss an item and undo with ⌘Z.
  await expect(page.getByRole('heading', { name: 'Needs me' })).toBeVisible();
  for (let i = 0; i < 12 && !/proposed subtasks/.test(await selectedNeed(page)); i++) await page.keyboard.press('ArrowDown');
  expect(await selectedNeed(page)).toMatch(/Worktree cleanup research/);
  await page.keyboard.press('Enter');
  await expect(toast).toContainText('Added 3 subtasks to #150');
  await expect(page.getByRole('option', { name: /Worktree cleanup research/ })).toHaveCount(0);
  const before = await page.locator('#app [role="listbox"] [role="option"]').count();
  // A second press within 400 ms of an action is ignored (so it can't act on the next item).
  await page.waitForTimeout(450);
  await page.keyboard.press('Meta+Backspace');
  await expect(toast).toContainText(/Dismissed #\d+/);
  await expect(page.locator('#app [role="listbox"] [role="option"]')).toHaveCount(before - 1);
  await page.keyboard.press('Meta+z');
  await expect(toast).toContainText('Undid dismiss');
  await expect(page.locator('#app [role="listbox"] [role="option"]')).toHaveCount(before);

  // 2. G A, ↓↓, Space, S 4 with a reason, then ⌥⌘↑.
  await page.keyboard.press('g');
  await page.keyboard.press('a');
  await expect(page.getByRole('heading', { name: 'All tasks' })).toBeVisible();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  const tree = page.getByRole('tree');
  await expect(tree).toHaveAttribute('aria-activedescendant', 'row-144');
  await page.keyboard.press(' ');
  await expect(toast).toContainText('Marked #144 done');
  await page.keyboard.press('s');
  await page.keyboard.press('4');
  await page.getByLabel('Why is it blocked?').fill('Waiting for test keys');
  await page.keyboard.press('Enter');
  await expect(toast).toContainText('Marked #144 blocked');
  await expect(page.locator('[data-row="144"]')).toContainText('Blocked');
  await page.keyboard.press('Alt+Meta+ArrowUp');
  await expect(toast).toContainText('Moved #144 up');
  await expect(page.locator('[data-row]').nth(1)).toHaveAttribute('data-row', '144');

  // 3. ⌘K, type an id, ↩, then ⌘↓ for the page and its map.
  await page.keyboard.press('Meta+k');
  await page.keyboard.type('200');
  await page.keyboard.press('Enter');
  await expect(tree).toHaveAttribute('aria-activedescendant', 'row-200');
  await page.keyboard.press('Meta+ArrowDown');
  await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Refunds' })).toBeVisible();

  // 4. C, a title, ↓ into notes, Tab to a repo, ↩ (then Esc and ↩ to create).
  await page.keyboard.press('Meta+ArrowUp');
  await page.keyboard.press('c');
  await page.keyboard.type('Fix double booking');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.type('Two people can hold one slot.');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'New task' }).getByRole('button', { name: 'airboss' })).toBeVisible();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'New task' })).toHaveCount(0);
  const created = page.locator('[data-row]', { hasText: 'Fix double booking' });
  await expect(created).toBeVisible();
  await expect(created).toHaveAttribute('aria-selected', 'true');
  await expect(created).toContainText('airboss');

  // 5. ? for the sheet.
  await page.keyboard.press('Escape');
  await page.keyboard.press('?');
  await expect(page.getByRole('dialog', { name: 'Keyboard shortcuts' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Keyboard shortcuts' })).toHaveCount(0);

  const unscripted = await page.evaluate(() => (window as unknown as { __airbossMock: { calls: { command: string }[] } }).__airbossMock.calls.length);
  expect(unscripted).toBeGreaterThan(10);
  expect(errors).toEqual([]);
});
