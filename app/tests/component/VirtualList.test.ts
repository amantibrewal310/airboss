import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { createRawSnippet, tick } from 'svelte';
import VirtualList from '../../src/lib/components/VirtualList.svelte';

type Item = { key: string; n: number };
const items: Item[] = Array.from({ length: 2000 }, (_, n) => ({ key: `r${n}`, n }));
const item = createRawSnippet((it: () => Item) => ({ render: () => `<div class="r" data-n="${it().n}">Row ${it().n}</div>` }));

function host(height: number) {
  const el = document.createElement('div');
  el.style.height = `${height}px`;
  el.style.position = 'relative';
  document.body.append(el);
  return el;
}

describe('VirtualList', () => {
  it('renders 2,000 rows with fewer than 60 in the DOM', async () => {
    const screen = await render(VirtualList<Item>, { target: host(812), props: { items, heightOf: () => 38, item } });
    await expect.element(screen.getByText('Row 0')).toBeInTheDocument();
    expect(document.querySelectorAll('.r').length).toBeLessThan(60);
    expect(document.querySelectorAll('.r').length).toBeGreaterThan(20);
  });

  it('keeps the active row rendered and scrolled into view', async () => {
    const screen = await render(VirtualList<Item>, { target: host(400), props: { items, heightOf: () => 38, item, active: 1500 } });
    await expect.element(screen.getByText('Row 1500')).toBeInTheDocument();
    const row = document.querySelector('[data-n="1500"]')!.getBoundingClientRect();
    const box = document.querySelector('.scroller')!.getBoundingClientRect();
    expect(row.top).toBeGreaterThanOrEqual(box.top);
    expect(row.bottom).toBeLessThanOrEqual(box.bottom + 1);
    expect(document.querySelectorAll('.r').length).toBeLessThan(60);
  });

  it('follows a fast fling on the scroll event itself, not a frame later', async () => {
    const screen = await render(VirtualList<Item>, { target: host(400), props: { items, heightOf: () => 38, item } });
    await expect.element(screen.getByText('Row 0')).toBeInTheDocument();
    const scroller = document.querySelector<HTMLElement>('.scroller')!;
    scroller.scrollTop = 38 * 1200;
    scroller.dispatchEvent(new Event('scroll'));
    await tick();
    expect(document.querySelector('[data-n="1200"]')).not.toBeNull();
    expect(document.querySelectorAll('.r').length).toBeLessThan(40);
  });
});
