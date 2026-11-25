import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import RowHost from './RowHost.svelte';
import Harness from './Harness.svelte';
import NeedsInbox from '../../src/windows/main/NeedsInbox.svelte';
import { buildRows } from '../../src/lib/mock/viewmodel';
import { stateFromSeed } from '../../src/lib/mock/model';
import { loadFixture } from '../../src/lib/mock/fixtures';
import { startApp, wait } from './setup';
import '../../src/styles/tokens.css';
import '../../src/styles/base.css';

const seed = loadFixture('design');
const { rows } = buildRows(stateFromSeed(seed));
const now = Date.parse(seed.now);
const hover = (el: Element) => el.dispatchEvent(new PointerEvent('pointerenter'));

/** P2-16, P1-13: a tooltip repeats text only when the text is cut off. */
describe('tooltips only when truncated', () => {
  it('row titles', async () => {
    const wide = await render(RowHost, { row: rows.get(144)!, width: 900, repos: seed.repos, now });
    const t = wide.container.querySelector<HTMLElement>('[data-row="144"] .t')!;
    hover(t);
    expect(t.hasAttribute('title')).toBe(false);
    wide.unmount();
    const narrow = await render(RowHost, { row: rows.get(144)!, width: 300, repos: seed.repos, now });
    const n = narrow.container.querySelector<HTMLElement>('[data-row="144"] .t')!;
    expect(n.scrollWidth).toBeGreaterThan(n.clientWidth);
    hover(n);
    expect(n.title).toBe('Web: slot picker');
  });

  it('Needs me: inbox lines, and the card’s one-line meta', async () => {
    const { app } = await startApp('design');
    const screen = await render(Harness, { app, component: NeedsInbox });
    await wait(30);
    const tt = screen.container.querySelector<HTMLElement>('.ib .tt')!;
    hover(tt);
    expect(tt.hasAttribute('title')).toBe(false);
    const l2 = screen.container.querySelector<HTMLElement>('.ib .l2')!;
    hover(l2);
    expect(l2.hasAttribute('title')).toBe(false);

    const k = screen.container.querySelector<HTMLElement>('.detail .k')!;
    expect(k.getBoundingClientRect().height).toBeLessThanOrEqual(16);
    const task = k.querySelector<HTMLElement>('.task')!;
    hover(task);
    expect(task.hasAttribute('title')).toBe(false);
    // Squeeze the card: the meta stays on one line, cut with an ellipsis, and now has a tooltip.
    screen.container.querySelector<HTMLElement>('.detail')!.style.width = '200px';
    screen.container.querySelector<HTMLElement>('.detail')!.style.flex = 'none';
    await wait(20);
    expect(k.getBoundingClientRect().height).toBeLessThanOrEqual(16);
    expect(task.scrollWidth).toBeGreaterThan(task.clientWidth);
    hover(task);
    expect(task.title).toBe('144 Web: slot picker');
  });
});
