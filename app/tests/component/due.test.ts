import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import NeedsInbox from '../../src/windows/main/NeedsInbox.svelte';
import { startApp, wait } from './setup';

/** Task 59: the Due card (fixture `due`: 151 Rotate local MCP token was due yesterday; 206 is due in 3 days). */
async function open() {
  const { app, backend } = await startApp('due');
  app.go('needs');
  const item = app.needs.visible.find((n) => n.kind === 'due')!;
  expect(item).toBeTruthy();
  app.selectNeed(item.key);
  const screen = await render(Harness, { app, component: NeedsInbox });
  await wait(30);
  return { app, backend, item, screen };
}

const buttons = (root: HTMLElement) => [...root.querySelectorAll<HTMLButtonElement>('.detail .acts button')];
const label = (b: HTMLButtonElement) => b.textContent!.replace(/\s+/g, ' ').trim();

describe('Due', () => {
  it('lists only the task whose day has come, after Stopped and before Proposed', async () => {
    const { app, item, screen } = await open();
    expect(app.needs.visible.filter((n) => n.kind === 'due').map((n) => n.task)).toEqual([151]);
    expect(item.headline).toBe('Due yesterday');
    expect(item.key).toBe(`due:151:${app.tasks.rows.get(151)!.due_at}`);
    const heads = [...screen.container.querySelectorAll('.ib-h span:first-child')].map((e) => e.textContent);
    expect(heads.indexOf('Due')).toBeGreaterThan(-1);
    expect(heads.indexOf('Proposed')).toBe(heads.indexOf('Due') + 1);
    const card = screen.container.querySelector<HTMLElement>('.detail .big')!;
    expect(card.querySelector('.q')!.textContent).toBe('Due yesterday');
    expect(card.querySelector('.kic svg path')!.getAttribute('d')).toContain('M2.5 6.5h11');
  });

  it('offers Open, Mark done, Snooze and Dismiss, with their keys', async () => {
    const { screen } = await open();
    expect(buttons(screen.container).map(label)).toEqual(['Open ↩', 'Mark done Space', 'Snooze 3 days', 'Dismiss ⌘⌫']);
  });

  it('Mark done sets the task done as me, and the item leaves', async () => {
    const { app, backend, item, screen } = await open();
    buttons(screen.container)[1]!.click();
    await wait(300);
    expect(backend.calls.filter((c) => c.command === 'set_status').map((c) => c.args)).toEqual([{ id: 151, status: 'done', note: null }]);
    await wait(100);
    expect(app.needs.items.some((n) => n.key === item.key)).toBe(false);
  });

  it('the card shows the date in Props, and moving it to a later day takes the item away', async () => {
    const { app, backend, item, screen } = await open();
    const tag = screen.container.querySelector<HTMLElement>('.task-sec .dep.late')!;
    expect(tag).toBeTruthy();
    expect(tag.textContent!.trim()).toMatch(/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) \d+ [A-Z][a-z]{2}/);
    const later = app.tasks.rows.get(206)!.due_at!;
    expect(await app.setDue(151, later)).toBeNull();
    expect(backend.calls.filter((c) => c.command === 'set_due').map((c) => c.args)).toEqual([{ id: 151, due: later }]);
    await wait(200);
    expect(app.tasks.rows.get(151)!.due_at).toBe(later);
    expect(app.needs.items.some((n) => n.key === item.key)).toBe(false);
  });

  it('a date that has come puts a task in Needs me; clearing it takes it out', async () => {
    const { app } = await startApp('due');
    const today = app.tasks.rows.get(151)!.due_at!;
    expect(await app.setDue(205, today)).toBeNull();
    await wait(200);
    expect(app.needs.items.find((n) => n.task === 205)?.kind).toBe('due');
    expect(await app.setDue(205, '')).toBeNull();
    await wait(200);
    expect(app.tasks.rows.get(205)!.due_at).toBeNull();
    expect(app.needs.items.some((n) => n.task === 205)).toBe(false);
  });

  it('refuses a date that isn’t a real day', async () => {
    const { app } = await startApp('due');
    expect(await app.setDue(205, '2026-02-30')).toBe('A due date is a real day, like 2026-12-04.');
    expect(app.tasks.rows.get(205)!.due_at).toBeNull();
  });
});
