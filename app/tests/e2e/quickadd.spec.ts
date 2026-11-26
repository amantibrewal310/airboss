import { expect, test, type Page } from '@playwright/test';

/**
 * Quick add laid out as in the app (`?native`: the window is the card) in a viewport that plays
 * the panel window: it is resized to whatever height the page asks Rust for (`quickadd_resize`).
 * With any number of repos or parents, an open picker must lie wholly inside the window, show
 * about 8 rows and scroll past them; when it closes, the window is the card again.
 */

const WIDTH = 680;
const ROW = 30;

/** Records quick add's resize calls and gives it `repos` repos. */
async function open(page: Page, repos: number) {
  await page.addInitScript((n) => {
    const w = window as unknown as { __qa: { heights: number[] }; __airbossMock?: unknown };
    w.__qa = { heights: [] };
    let backend: { invoke: (c: string, a: unknown) => Promise<unknown> } | undefined;
    Object.defineProperty(window, '__airbossMock', {
      configurable: true,
      get: () => backend,
      set(b: { invoke: (c: string, a: unknown) => Promise<unknown> }) {
        backend = b;
        const invoke = b.invoke.bind(b);
        b.invoke = async (command, args) => {
          if (command === 'quickadd_resize') w.__qa.heights.push((args as { height: number }).height);
          const res = await invoke(command, args);
          if (command === 'quickadd_data') {
            const d = res as { repos: Record<string, unknown>[] };
            const first = d.repos[0]!;
            d.repos = Array.from({ length: n }, (_, i) => ({ ...first, id: 900 + i, name: `repo-${i + 1}`, path: `/tmp/repo-${i + 1}` }));
          }
          return res;
        };
      },
    });
  }, repos);
  await page.setViewportSize({ width: WIDTH, height: 320 });
  await page.goto('/quickadd.html?fixture=design&native');
  await expect(page.getByRole('textbox', { name: 'Title' })).toBeFocused();
  await follow(page);
}

/** Plays the window: takes the height the page last asked for. */
async function follow(page: Page): Promise<number> {
  await page.waitForTimeout(120);
  const heights = await page.evaluate(() => (window as unknown as { __qa: { heights: number[] } }).__qa.heights);
  const h = heights.at(-1)!;
  expect(h).toBeGreaterThan(100);
  await page.setViewportSize({ width: WIDTH, height: h });
  return h;
}

async function cardBottom(page: Page) {
  return page.locator('.card').evaluate((el) => Math.ceil(el.getBoundingClientRect().bottom));
}

/** The open picker is wholly in the window and shows min(n, ~8) whole rows. */
async function pickerFits(page: Page, options: number) {
  const dd = page.locator('.dd');
  await expect(dd).toBeVisible();
  await page.waitForTimeout(250); // its drop-in animation
  const h = await follow(page);
  const box = await dd.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const opts = el.querySelector('.opts')!;
    return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, shown: opts.clientHeight, all: opts.scrollHeight };
  });
  expect(box.top).toBeGreaterThanOrEqual(0);
  expect(box.left).toBeGreaterThanOrEqual(0);
  expect(box.right).toBeLessThanOrEqual(WIDTH);
  expect(box.bottom).toBeLessThanOrEqual(h);
  const fullRows = Math.floor(box.shown / ROW);
  expect(fullRows).toBeGreaterThanOrEqual(Math.min(options, 8));
  if (options > 9) expect(box.all).toBeGreaterThan(box.shown);
  // The last row can be scrolled to and is then inside the window too.
  const last = page.locator('.dd [role="option"]').last();
  await last.scrollIntoViewIfNeeded();
  const lastBox = await last.boundingBox();
  expect(lastBox!.y + lastBox!.height).toBeLessThanOrEqual(h);
}

for (const repos of [1, 4, 12, 40]) {
  test(`quick add: the Repos picker with ${repos} repos lies inside the window`, async ({ page }) => {
    await open(page, repos);
    const closed = await follow(page);
    expect(closed).toBe(await cardBottom(page));
    await page.keyboard.press('Tab');
    await pickerFits(page, repos);
    await expect(page.locator('.dd [role="option"]')).toHaveCount(repos);
    // Picking keeps it open (a multi-select); Esc closes it and the window is the card again.
    await page.keyboard.press('Enter');
    await expect(page.locator('.dd [role="option"]').first()).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Escape');
    await expect(page.locator('.dd')).toHaveCount(0);
    expect(await follow(page)).toBe(await cardBottom(page));
  });
}

test('quick add: the Parent picker lies inside the window and scrolls', async ({ page }) => {
  await open(page, 3);
  await page.keyboard.press('Shift+Tab');
  const count = await page.locator('.dd [role="option"]').count();
  expect(count).toBeGreaterThan(9);
  await pickerFits(page, count);
  // Filtering shrinks it, and the window with it.
  const before = await follow(page);
  await page.keyboard.type('Refund');
  await page.waitForTimeout(250);
  const after = await follow(page);
  expect(after).toBeLessThan(before);
  await pickerFits(page, await page.locator('.dd [role="option"]').count());
  await page.keyboard.press('Escape');
  expect(await follow(page)).toBe(await cardBottom(page));
});
