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
  expect(vars.text3).toBe('#b0b2ba');
  // The minifier writes rgba(255,255,255,.2) as #fff3.
  expect(['#fff3', 'rgba(255, 255, 255, 0.2)']).toContain(vars.line);
});

const TEXT_TOKENS = ['--text', '--text-2', '--text-3', '--needs-text', '--ok-text', '--warn-text', '--link'];
const SURFACES = ['--bg', '--bg-side', '--bg-raised', '--bg-input'];

function luminance(hex: string): number {
  const h = hex.replace('#', '');
  const full = h.length === 3 ? [...h].map((c) => c + c).join('') : h.slice(0, 6);
  const [r = 0, g = 0, b = 0] = [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
const ratio = (a: string, b: string) => {
  const hi = Math.max(luminance(a), luminance(b));
  const lo = Math.min(luminance(a), luminance(b));
  return (hi + 0.05) / (lo + 0.05);
};

for (const scheme of ['dark', 'light'] as const) {
  test(`Increase Contrast never lowers a text colour's contrast (${scheme})`, async ({ page }) => {
    const read = async (contrast: 'more' | 'no-preference') => {
      await page.emulateMedia({ colorScheme: scheme, contrast });
      await page.goto('/?fixture=design&view=all&select=144');
      return page.evaluate((names) => {
        const s = getComputedStyle(document.documentElement);
        return Object.fromEntries(names.map((n) => [n, s.getPropertyValue(n).trim()])) as Record<string, string>;
      }, [...TEXT_TOKENS, ...SURFACES]);
    };
    const normal = await read('no-preference');
    const more = await read('more');
    for (const t of TEXT_TOKENS) {
      for (const bg of SURFACES) {
        const [n, m, nb, mb] = [normal[t] ?? '', more[t] ?? '', normal[bg] ?? '', more[bg] ?? ''];
        if (!nb.startsWith('#') || !mb.startsWith('#')) continue;
        expect(ratio(m, mb), `${t} on ${bg}`).toBeGreaterThanOrEqual(ratio(n, nb) - 0.01);
        expect(ratio(m, mb), `${t} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
}

test('every row has an accessible name and the list is a tree with levels', async ({ page }) => {
  await page.goto('/?fixture=design&view=all');
  const tree = page.getByRole('tree', { name: 'All tasks' });
  await expect(tree).toBeVisible();
  const item = page.getByRole('treeitem', { name: /^144, Web: slot picker, In progress, waiting on you, needs you, booking-web/ });
  await expect(item).toHaveAttribute('aria-level', '2');
  await expect(page.getByRole('treeitem', { name: /^142, Booking flow/ })).toHaveAttribute('aria-expanded', 'true');
});

test('Needs me items are named by kind, task and what Return does', async ({ page }) => {
  await page.goto('/?fixture=design');
  const first = page.getByRole('listbox', { name: 'Needs me' }).getByRole('option').first();
  await expect(first).toHaveAccessibleName(/^Question: 144 Web: slot picker\. .+ Return: Open thread$/);
});

test('progress rings say how much is done, and decorative ones stay silent', async ({ page }) => {
  await page.goto('/?fixture=design&view=all');
  await expect(page.locator('nav').getByRole('img', { name: '1 of 5 done' })).toHaveCount(1);
  // Rows read their progress in the row sentence; the ring itself is hidden.
  await expect(page.locator('[data-row="142"] .prog [role="img"]')).toHaveCount(0);
  await expect(page.locator('[data-row="142"] .prog')).toHaveAttribute('aria-hidden', 'true');
});

test('plan cards hold no controls inside an option', async ({ page }) => {
  await page.goto('/?fixture=design&view=plans');
  await expect(page.locator('[role="option"]')).toHaveCount(0);
  const card = page.getByRole('article', { name: 'Booking flow' });
  await expect(card).toBeFocused();
  await expect(card).toHaveAccessibleDescription(/^20% done, 1 of 5, (next up|nothing ready)/);
});
