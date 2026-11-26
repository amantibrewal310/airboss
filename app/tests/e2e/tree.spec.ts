import { expect, test, type Locator } from '@playwright/test';

/**
 * Hands-on QA #16: WebKit exposed All tasks as a plain group with group rows, so VoiceOver had no
 * tree, level or position. WebKit only accepts a tree (`AccessibilityTree::isTreeValid`) when every
 * element between it and its treeitems is a `group` or `presentation`/`none`, and demotes it to a
 * group otherwise; its treeitems then lose their role too.
 */

/** WebKit's rule, run on the page: the elements between a composite and its items that break it. */
async function breakers(list: Locator, item: string): Promise<string[]> {
  return list.evaluate((root, item) => {
    const ok = new Set(['group', 'presentation', 'none']);
    const bad: string[] = [];
    const queue = [...root.children];
    while (queue.length) {
      const el = queue.shift()!;
      const role = el.getAttribute('role') ?? '';
      if (role === item) continue;
      if (!ok.has(role)) bad.push(`<${el.tagName.toLowerCase()} class="${el.className}" role="${role}">`);
      queue.push(...el.children);
    }
    return bad;
  }, item);
}

/** The elements between a composite and each of its items that have no role (or the wrong one). */
async function wrappers(list: Locator, item: string): Promise<string[]> {
  return list.evaluate((root, item) => {
    const ok = new Set(['group', 'presentation', 'none']);
    const bad = new Set<string>();
    for (const el of root.querySelectorAll(`[role="${item}"]`)) {
      for (let p = el.parentElement; p && p !== root; p = p.parentElement) {
        if (!ok.has(p.getAttribute('role') ?? '')) bad.add(`<${p.tagName.toLowerCase()} class="${p.className}">`);
      }
    }
    return [...bad];
  }, item);
}

test('All tasks (Tree) is a valid tree: treeitems with a level, nothing roleless in between', async ({ page }) => {
  await page.goto('/?fixture=design&view=all');
  const tree = page.getByRole('tree', { name: 'All tasks' });
  await expect(tree).toBeVisible();
  // Playwright's own reading: a tree of treeitems at levels 1 and 2.
  await expect(tree).toMatchAriaSnapshot(`
    - tree "All tasks":
      - treeitem /142, Booking flow/ [expanded] [level=1]
      - treeitem /143, API/ [level=2]
  `);
  expect(await tree.getByRole('treeitem').count()).toBeGreaterThan(5);
  // WebKit's stricter reading, which decides whether VoiceOver gets an outline at all.
  expect(await breakers(tree, 'treeitem')).toEqual([]);
  expect(await wrappers(tree, 'treeitem')).toEqual([]);
  // The rows carry their level and position themselves (the tree is flat in the DOM).
  const first = tree.getByRole('treeitem').first();
  await expect(first).toHaveAttribute('aria-level', '1');
  await expect(first).toHaveAttribute('aria-posinset', '1');
  await expect(first).toHaveAttribute('aria-setsize', /\d+/);
});

test('All tasks (By status) is a listbox of options, with nothing roleless in between and the sticky header outside', async ({ page }) => {
  await page.goto('/?fixture=design&view=all');
  await page.getByRole('radio', { name: 'By status' }).click();
  const list = page.getByRole('listbox', { name: 'All tasks' });
  await expect(list).toBeVisible();
  expect(await list.getByRole('option').count()).toBeGreaterThan(5);
  expect(await wrappers(list, 'option')).toEqual([]);
  // Scroll so the sticky group header shows: it's a sibling of the list, hidden from the tree.
  await list.evaluate((el) => (el.scrollTop = 200));
  const sticky = page.locator('.sticky');
  await expect(sticky).toBeVisible();
  await expect(sticky).toHaveAttribute('aria-hidden', 'true');
  expect(await list.locator('.sticky').count()).toBe(0);
});
