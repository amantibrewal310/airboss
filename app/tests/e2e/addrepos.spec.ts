import { expect, test } from '@playwright/test';

/**
 * Add repos with many T3 Code projects in a short window (the owner's report, 26 Nov 2026): the
 * list grew past the window and took "Add another folder…" and "Add N repos" with it. Now the
 * list scrolls inside the modal, and only a path that's cut off gets a tooltip.
 */

const HEIGHT = 600;

test('a long Add repos list scrolls inside the modal, with its buttons in the window', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: HEIGHT });
  await page.goto('/?fixture=design');
  await page.evaluate(() => {
    type Mock = { s: { t3Projects: { id: string; name: string; path: string }[]; gitFolders: string[] } };
    const mock = (globalThis as unknown as { __airbossMock: Mock }).__airbossMock;
    const add = (id: string, name: string, path: string) => {
      mock.s.t3Projects.push({ id, name, path });
      mock.s.gitFolders.push(path);
    };
    for (let i = 0; i < 24; i++) add(`p-many-${i}`, `project-${i}`, `~/Workspace/Work/Projects/project-${i}`);
    add('p-long', 'long', `~/Workspace/Work/Projects/${'a-very-long-folder-name-'.repeat(6)}end`);
  });
  await page.getByRole('button', { name: 'Add repos' }).click();
  const dialog = page.getByRole('dialog', { name: 'Add repos' });
  const rows = dialog.getByRole('checkbox');
  await expect(rows.first()).toBeVisible();
  expect(await rows.count()).toBeGreaterThan(24);

  // The modal ends inside the window, and its buttons are on screen.
  const box = (await dialog.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(HEIGHT);
  await expect(dialog.getByRole('button', { name: /Add another folder/ })).toBeInViewport({ ratio: 1 });

  // The list scrolls: End reaches the last row and brings it into view.
  await rows.first().focus();
  await page.keyboard.press('End');
  const last = rows.last();
  await expect(last).toBeFocused();
  await expect(last).toBeInViewport({ ratio: 1 });
  await page.keyboard.press('Space');
  await expect(dialog.getByRole('button', { name: 'Add 1 repo' })).toBeInViewport({ ratio: 1 });

  // A path that fits has no tooltip to cover the next row; one that's cut off shows in full.
  const short = dialog.locator('.path', { hasText: '~/Workspace/Work/Projects/project-0' }).first();
  await short.hover();
  await expect(short).not.toHaveAttribute('title');
  await expect(dialog.getByRole('checkbox', { name: 'project-0' })).not.toHaveAttribute('title');
  const long = dialog.locator('.path', { hasText: 'a-very-long-folder-name' });
  await long.scrollIntoViewIfNeeded();
  await long.hover();
  await expect(long).toHaveAttribute('title', /end$/);
});
