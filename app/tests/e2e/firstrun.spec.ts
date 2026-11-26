import { expect, test, type Page } from '@playwright/test';

/**
 * First run on the mock (hands-on QA #9): the Agents row goes Not set up → Not connected →
 * Ready, and Settings › Agent setup shows each tool.
 */

function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' || /UnscriptedCall/.test(m.text())) errors.push(m.text());
  });
  return errors;
}

const agents = (page: Page) => page.locator('.c').filter({ hasText: 'Agents ·' });

test('first run connects Claude Code in one click once the CLI is installed', async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto('/?fixture=welcome');
  const footer = page.locator('.conn > button');
  await expect(agents(page)).toContainText('Agents · Not set up');
  // By keyboard: WebKit doesn't focus a clicked button, and focus should stay on this one.
  await agents(page).getByRole('button', { name: 'Install command-line tool' }).focus();
  await page.keyboard.press('Enter');

  await expect(agents(page)).toContainText('Agents · Not connected');
  await expect(agents(page)).toContainText("Agents can't report to Airboss yet.");
  await expect(footer).toHaveText('Agents not connected');
  await expect(agents(page).getByRole('button', { name: 'Connect Claude Code' })).toBeFocused();
  await page.keyboard.press('Enter');

  await expect(page.locator('[data-toast]').last()).toContainText('Connected Claude Code');
  await expect(agents(page)).toContainText('Agents · Ready');
  await expect(agents(page)).toContainText('No agent has reported yet.');
  await expect(agents(page).getByRole('button', { name: 'Copy agent note' })).toBeFocused();
  await expect(footer).toHaveText('All connected');

  // Settings › Agent setup agrees: Claude Code connected, Codex by its snippet.
  await page.keyboard.press('Meta+Comma');
  const tools = page.getByRole('list', { name: 'Agent tools' });
  await expect(tools.getByRole('listitem').filter({ hasText: 'Claude Code' })).toContainText('Connected');
  await expect(tools.getByRole('listitem').filter({ hasText: 'Codex' }).getByRole('button', { name: 'Copy Codex setup' })).toBeVisible();
  expect(errors).toEqual([]);
});

test('Settings connects Claude Code from its line', async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto('/?fixture=welcome');
  await agents(page).getByRole('button', { name: 'Install command-line tool' }).click();
  await expect(agents(page)).toContainText('Not connected');
  await page.keyboard.press('Meta+Comma');
  const claude = page.getByRole('list', { name: 'Agent tools' }).getByRole('listitem').filter({ hasText: 'Claude Code' });
  await expect(claude).toContainText('Not connected');
  await claude.getByRole('button', { name: 'Connect' }).click();
  await expect(claude).toContainText('Connected');
  await expect(claude.getByRole('button')).toHaveCount(0);
  expect(errors).toEqual([]);
});
