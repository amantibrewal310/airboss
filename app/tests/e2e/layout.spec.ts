import { expect, test, type Page } from '@playwright/test';

/**
 * The narrow steps, measured at the two window widths the app is checked at: 1100px (a small
 * MacBook window) and 1440px (the design's width).
 */

const width = (page: Page, selector: string) => page.locator(selector).first().evaluate((el) => el.getBoundingClientRect().width);
const shown = (page: Page, selector: string) =>
  page.locator(selector).first().evaluate((el) => getComputedStyle(el).display !== 'none' && el.getBoundingClientRect().width > 0);

test.describe('at 1100px', () => {
  test.use({ viewport: { width: 1100, height: 760 } });

  test('the panel narrows to 380px so the list keeps its chips', async ({ page }) => {
    await page.goto('/?fixture=design&view=all&select=144');
    await page.locator('[data-row="144"]').waitFor();
    expect(await width(page, 'aside.peek')).toBeCloseTo(380, 0);
    expect(await width(page, '.list')).toBeGreaterThanOrEqual(480);
    // The row's chip keeps its label.
    expect(await shown(page, '[data-row="144"] .sig .lab')).toBe(true);
    expect(await page.locator('[data-row="144"] .sig .lab').first().textContent()).toBe('Needs you');
  });

  test('plan cards stay at least 340px wide', async ({ page }) => {
    await page.goto('/?fixture=design&view=plans');
    await page.locator('.plan').first().waitFor();
    expect(await width(page, '.plan')).toBeGreaterThanOrEqual(340);
  });

  test('the run map narrows its columns to about 130px so four waves fit', async ({ page }) => {
    await page.goto('/?fixture=design&view=all&select=200');
    await page.keyboard.press('Meta+ArrowDown');
    await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
    const column = await page.locator('.ro .colh').evaluateAll((els) => els[1]!.getBoundingClientRect().left - els[0]!.getBoundingClientRect().left);
    expect(column).toBeGreaterThanOrEqual(120);
    expect(column).toBeLessThanOrEqual(130);
    const scroll = page.locator('.map-scroll');
    expect(await scroll.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    await expect(scroll).not.toHaveClass(/more-r/);
  });
});

test.describe('at 1000px', () => {
  test.use({ viewport: { width: 1000, height: 760 } });

  test('a row’s chip keeps its icon and count when its label goes', async ({ page }) => {
    await page.goto('/?fixture=design&view=all&select=144');
    await page.locator('[data-row="144"]').waitFor();
    expect(await width(page, '.list')).toBeLessThanOrEqual(460);
    expect(await shown(page, '[data-row="144"] .sig .lab')).toBe(false);
    expect(await shown(page, '[data-row="144"] .sig svg')).toBe(true);
    expect(await shown(page, '[data-row="144"] .sig .n')).toBe(true);
    // Ready to start is no chip: the Ready view lists it.
    expect(await page.locator('[data-row="146"] .sig').count()).toBe(0);
  });

  test('a run map wider than the page fades its hidden edge and says it scrolls', async ({ page }) => {
    await page.goto('/?fixture=design&view=all&select=200');
    await page.keyboard.press('Meta+ArrowDown');
    await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
    const scroll = page.locator('.map-scroll');
    expect(await scroll.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
    await expect(scroll).toHaveClass(/more-r/);
    await expect(scroll).not.toHaveClass(/more-l/);
    expect(await scroll.evaluate((el) => getComputedStyle(el).maskImage)).toContain('linear-gradient');
    await expect(page.locator('.map-scroll + .legend')).toContainText('Scroll sideways for more');
    await scroll.evaluate((el) => el.scrollTo({ left: el.scrollWidth }));
    await expect(scroll).toHaveClass(/more-l/);
    await expect(scroll).not.toHaveClass(/more-r/);
  });
});

test.describe('at 1440px', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test('the panel is 480px and the list keeps repo pills and full chip labels', async ({ page }) => {
    await page.goto('/?fixture=design&view=all&select=144');
    await page.locator('[data-row="144"]').waitFor();
    expect(await width(page, 'aside.peek')).toBeCloseTo(480, 0);
    expect(await shown(page, '[data-row="144"] .rn')).toBe(true);
    expect(await shown(page, '[data-row="144"] .sig .lab')).toBe(true);
    // A parent shows no repo tags: they'd repeat on every phase.
    expect(await page.locator('[data-row="142"] .rn').count()).toBe(0);
  });

  test('the run map fits without scrolling', async ({ page }) => {
    await page.goto('/?fixture=design&view=all&select=200');
    await page.keyboard.press('Meta+ArrowDown');
    await expect(page.getByRole('toolbar', { name: /Run-order map/ })).toBeVisible();
    const scroll = page.locator('.map-scroll');
    expect(await scroll.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    await expect(scroll).not.toHaveClass(/more-r/);
    await expect(page.locator('.map-scroll + .legend')).not.toContainText('Scroll sideways');
  });
});
