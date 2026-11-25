import { expect, test } from '@playwright/test';

/** Reduce Motion and Increase Contrast visibly apply (SPEC-app "Accessibility"). */

test('Reduce Motion turns off every animation; the live pulse becomes a static ring', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/?fixture=full&view=all&select=163');
  await page.getByRole('tree').focus();
  await page.keyboard.press('g');
  await page.keyboard.press('p');
  const chip = page.locator('.live.working i').first();
  await expect(chip).toBeVisible();
  const pulse = await chip.evaluate((el) => {
    const s = getComputedStyle(el, '::after');
    return { name: s.animationName, opacity: s.opacity };
  });
  expect(pulse.name).toBe('none');
  expect(Number(pulse.opacity)).toBeGreaterThan(0);
  const panel = await page.locator('aside[data-zone="panel"]').evaluate((el) => getComputedStyle(el).animationName);
  expect(panel).toBe('none');
});

test('motion stays on without the preference (a short ease-out)', async ({ page }) => {
  await page.goto('/?fixture=full&view=progress');
  const name = await page.locator('.live.working i').first().evaluate((el) => getComputedStyle(el, '::after').animationName);
  expect(name).not.toBe('none');
});

test('Increase Contrast strengthens hairlines, text-3 and the keyboard outline', async ({ page }) => {
  await page.emulateMedia({ contrast: 'more', colorScheme: 'dark' });
  await page.goto('/?fixture=design&view=all&select=144');
  const vars = await page.evaluate(() => {
    const s = getComputedStyle(document.documentElement);
    return { line: s.getPropertyValue('--line').trim(), text3: s.getPropertyValue('--text-3').trim(), kb: s.getPropertyValue('--kb-outline').trim() };
  });
  expect(vars.kb).toBe('2px');
  expect(vars.text3).toBe('#9a9ca5');
  // The minifier writes rgba(255,255,255,.2) as #fff3.
  expect(['#fff3', 'rgba(255, 255, 255, 0.2)']).toContain(vars.line);
});

test('every row has an accessible name and the list is a tree with levels', async ({ page }) => {
  await page.goto('/?fixture=design&view=all');
  const tree = page.getByRole('tree', { name: 'All tasks' });
  await expect(tree).toBeVisible();
  const item = page.getByRole('treeitem', { name: /^144, Web: slot picker, In progress, waiting on you, needs you, booking-web/ });
  await expect(item).toHaveAttribute('aria-level', '2');
  await expect(page.getByRole('treeitem', { name: /^142, Booking flow/ })).toHaveAttribute('aria-expanded', 'true');
});
