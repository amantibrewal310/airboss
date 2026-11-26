import { expect, test, type Page } from '@playwright/test';

/**
 * First run on the mock (hands-on QA #9, #3, #2): the Agents row goes Not set up → Not
 * connected → Ready, Settings › Agent setup shows each tool, and the Repos step survives an
 * added folder and is one Tab stop.
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

test('the Repos step stays after Add another folder…, with ticks and focus kept', async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto('/?fixture=welcome');
  const projects = page.getByRole('group', { name: 'T3 Code projects' });
  await projects.getByRole('checkbox', { name: 'booking-web' }).click();
  const another = page.getByRole('button', { name: 'Add another folder…' });
  await another.click();

  await expect(page.locator('[data-toast]').last()).toContainText('Added side-project');
  await expect(page.getByRole('heading', { name: 'Repos' })).toBeVisible();
  await expect(projects.getByRole('checkbox', { name: 'booking-web' })).toHaveAttribute('aria-checked', 'true');
  const added = projects.getByRole('checkbox', { name: 'side-project' });
  await expect(added).toHaveAttribute('aria-checked', 'true');
  await expect(added).toHaveAttribute('aria-disabled', 'true');
  await expect(another).toBeFocused();
  expect(errors).toEqual([]);
});

test('the project list is one Tab stop with ↑↓, Home, End and Space', async ({ page }) => {
  await page.goto('/?fixture=welcome');
  await page.getByRole('checkbox', { name: 'Notify me when an agent asks something or stops' }).focus();
  await page.keyboard.press('Tab');
  const first = page.getByRole('checkbox', { name: 'airboss', exact: true });
  await expect(first).toBeFocused();
  await page.keyboard.press('End');
  await expect(page.getByRole('checkbox', { name: 'personal-site', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('Space');
  await expect(page.getByRole('checkbox', { name: 'notes', exact: true })).toHaveAttribute('aria-checked', 'false');
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('Space');
  await expect(page.getByRole('checkbox', { name: 'booking-api', exact: true })).toHaveAttribute('aria-checked', 'true');
  await page.keyboard.press('Home');
  await expect(first).toBeFocused();
  // One more Tab leaves the list, however many projects there are.
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Add another folder…' })).toBeFocused();
});

test('first run without T3 Code lists Herdr folders and adds one by keyboard', async ({ page }) => {
  const errors = watchErrors(page);
  await page.goto('/?fixture=herdr');
  await expect(page.getByRole('heading', { name: 'From Herdr' })).toBeVisible();
  const list = page.getByRole('group', { name: 'From Herdr' });
  await expect(list.getByRole('checkbox')).toHaveCount(4);
  await expect(page.locator('.c').filter({ hasText: 'Herdr · Ready' })).toBeVisible();
  await expect(page.locator('.c').filter({ hasText: 'T3 Code ·' })).toHaveCount(0);
  await list.getByRole('checkbox', { name: 'booking-web' }).focus();
  await page.keyboard.press('Space');
  await page.keyboard.press('Meta+Enter');
  await expect(list.getByRole('checkbox', { name: 'booking-web' })).toContainText('Added');
  expect(errors).toEqual([]);
});
