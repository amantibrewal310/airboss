import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import RowHost from './RowHost.svelte';
import { buildRows } from '../../src/lib/mock/viewmodel';
import { stateFromSeed } from '../../src/lib/mock/model';
import { loadFixture } from '../../src/lib/mock/fixtures';
import '../../src/styles/tokens.css';
import '../../src/styles/base.css';

const seed = loadFixture('full');
const { rows } = buildRows(stateFromSeed(seed));
const now = Date.parse(seed.now);

async function show(id: number, width = 900, selected = false) {
  const screen = await render(RowHost, { row: rows.get(id)!, width, repos: seed.repos, now, selected });
  const el = screen.container.querySelector<HTMLElement>(`[data-row="${id}"]`)!;
  return { screen, el };
}

/** The row's accessible name: a VoiceOver-only sentence, with the terse visible text hidden. */
const name = (el: Element) => el.querySelector(':scope > .sr')!.textContent;

const visible = (el: Element | null) => !!el && getComputedStyle(el).display !== 'none';

describe('TaskRow', () => {
  it('is exactly 38px at every status', async () => {
    for (const id of [142, 143, 144, 145, 146, 203, 153, 172]) {
      const { el } = await show(id);
      expect(el.getBoundingClientRect().height).toBe(38);
    }
  });

  it('reads like a sentence to VoiceOver', async () => {
    const { el } = await show(144);
    expect(name(el)).toBe('144, Web: slot picker, In progress, waiting on you, needs you, booking-web, 6 minutes ago');
    // Nothing else in the row is read, so the sentence is not followed by "6m" and the chip text.
    const read = [...el.querySelectorAll(':scope > :not([aria-hidden="true"]):not(.sr)')].filter((c) => c.textContent?.trim());
    expect(read).toEqual([]);
    expect(el.hasAttribute('aria-label')).toBe(false);
  });

  it('draws each status by shape, with its name', async () => {
    const labels: Record<number, string> = { 143: 'Done', 144: 'In progress', 145: 'Waiting', 146: 'Todo', 203: 'Blocked', 204: 'In review', 153: 'Proposed', 172: 'Dropped' };
    for (const [id, label] of Object.entries(labels)) {
      const { el } = await show(Number(id));
      expect(name(el)).toContain(label);
    }
  });

  it('shows a parent’s progress, a blocked pill, waits-on and Ready', async () => {
    expect((await show(142)).el.textContent).toContain('1/5');
    const blocked = (await show(203)).el;
    expect(blocked.querySelector('.pill.warn')?.textContent).toBe('Blocked');
    expect((await show(147)).el.textContent).toContain('waits on 144, 145, 146');
    expect((await show(146)).el.querySelector('.pill.ok')?.textContent).toBe('Ready');
  });

  it('badges a task’s priority after its title, and says it to VoiceOver', async () => {
    const { el } = await show(163);
    const badge = el.querySelector<HTMLElement>('.pri');
    expect(badge?.textContent).toBe('P0');
    expect(badge?.title).toBe('Priority P0');
    expect(name(el)).toContain('priority P0');
    expect(el.getBoundingClientRect().height).toBe(38);
    expect(visible((await show(163, 340)).el.querySelector('.pri'))).toBe(true);
    // No priority, no badge.
    expect((await show(144)).el.querySelector('.pri')).toBeNull();
  });

  it('truncates a long title with an ellipsis and a tooltip, never wrapping', async () => {
    const { el } = await show(194, 700);
    const t = el.querySelector<HTMLElement>('.t')!;
    expect(t.scrollWidth).toBeGreaterThan(t.clientWidth);
    expect(getComputedStyle(t).textOverflow).toBe('ellipsis');
    // The tooltip is added on hover, and only because the title is cut off.
    expect(t.hasAttribute('title')).toBe(false);
    t.dispatchEvent(new PointerEvent('pointerenter'));
    expect(t.title).toContain('screen readers');
    expect(el.getBoundingClientRect().height).toBe(38);
  });

  it('drops detail in two steps at narrow widths', async () => {
    const wide = (await show(147, 900)).el;
    expect(visible(wide.querySelector('.rn'))).toBe(true);
    expect(visible(wide.querySelector('.rd'))).toBe(false);
    const mid = (await show(147, 680)).el;
    expect(visible(mid.querySelector('.rn'))).toBe(false);
    expect(visible(mid.querySelector('.rd'))).toBe(true);
    expect(visible(mid.querySelector('.long'))).toBe(true);
    const narrow = (await show(147, 520)).el;
    expect(visible(narrow.querySelector('.long'))).toBe(false);
    expect(visible(narrow.querySelector('.short'))).toBe(true);
    const parent = (await show(142, 520)).el;
    expect(visible(parent.querySelector('.pc'))).toBe(false);
    // Chips shorten first ("Waiting"), then lose their label at 460px or less.
    const live = (await show(144, 520)).el;
    expect(visible(live.querySelector('.live .full'))).toBe(false);
    expect(live.querySelector('.live .brief')!.textContent).toBe('Waiting');
    expect(visible(live.querySelector('.live .brief'))).toBe(true);
    expect(visible(live.querySelector('.dot'))).toBe(true);
    const tight = (await show(144, 440)).el;
    expect(visible(tight.querySelector('.live .lt'))).toBe(false);
    expect(visible(tight.querySelector('.dot'))).toBe(false);
    expect(live.querySelector('.live')!.getAttribute('title')).toBe('Waiting on you');
    expect(name(live)).toContain('waiting on you');
  });

  it('dims the needs-me dot of a stale item, with a tooltip', async () => {
    const { el } = await show(180);
    const dot = el.querySelector<HTMLElement>('.dot')!;
    expect(dot.title).toBe('May be out of date');
    expect(Number(getComputedStyle(dot).opacity)).toBeLessThan(1);
  });

  it('draws the keyboard outline on the selected row', async () => {
    const { el } = await show(144, 900, true);
    expect(getComputedStyle(el, '::after').boxShadow).toContain('inset');
  });
});
