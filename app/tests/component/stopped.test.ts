import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import NeedsInbox from '../../src/windows/main/NeedsInbox.svelte';
import { startApp, wait } from './setup';

/** R5: the Agent stopped card (fixture `stopped`: 202 Refund service, its agent settled 20 minutes ago). */
async function open() {
  const { app, backend } = await startApp('stopped');
  app.go('needs');
  const item = app.needs.visible.find((n) => n.kind === 'agent_stopped')!;
  expect(item).toBeTruthy();
  app.selectNeed(item.key);
  const screen = await render(Harness, { app, component: NeedsInbox });
  await wait(30);
  return { app, backend, item, screen };
}

const buttons = (root: HTMLElement) => [...root.querySelectorAll<HTMLButtonElement>('.detail .acts button')];
const label = (b: HTMLButtonElement) => b.textContent!.replace(/\s+/g, ' ').trim();

describe('Agent stopped', () => {
  it('sits right after Failed runs, with its icon and words', async () => {
    const { item, screen } = await open();
    const heads = [...screen.container.querySelectorAll('.ib-h span:first-child')].map((e) => e.textContent);
    expect(heads.indexOf('Stopped')).toBe(heads.indexOf('Failed runs') + 1);
    const row = screen.container.querySelector<HTMLElement>(`[id="need-${item.key}"]`)!;
    expect(row.querySelector('.sr')!.textContent).toBe('Agent stopped: 202 Refund service. refund-service stopped. Return: Open thread');
    expect(row.querySelector('.l2')!.textContent).toBe('refund-service stopped');
    const card = screen.container.querySelector<HTMLElement>('.detail .big')!;
    expect(card.querySelector('.lead')!.textContent).toMatch(/^Agent stopped · 20m ago ·$/);
    expect(card.querySelector('.q')!.textContent).toBe('Agent stopped · no PR yet');
    expect(card.querySelector('.kic svg path')!.getAttribute('d')).toContain('M6.5 6v4');
  });

  it('offers Open thread, Mark done, Set in review, Snooze and Dismiss, with their keys', async () => {
    const { screen } = await open();
    const b = buttons(screen.container);
    expect(b.map(label)).toEqual(['Open thread ↩', 'Mark done Space', 'Set in review', 'Snooze 3 days', 'Dismiss ⌘⌫']);
    expect(b[0]!.classList.contains('pri')).toBe(true);
  });

  it('Mark done sets the task done as me, and the item leaves', async () => {
    const { app, backend, item, screen } = await open();
    buttons(screen.container)[1]!.click();
    await wait(300);
    expect(backend.calls.filter((c) => c.command === 'set_status').map((c) => c.args)).toEqual([{ id: 202, status: 'done', note: null }]);
    expect(app.tasks.rows.get(202)!.status).toBe('done');
    expect(app.toasts.list.at(-1)!.text).toBe('Marked 202 done');
    await wait(100);
    expect(app.needs.items.some((n) => n.key === item.key)).toBe(false);
  });

  it('Set in review moves the task to in review, and the item leaves', async () => {
    const { app, backend, item, screen } = await open();
    buttons(screen.container)[2]!.click();
    await wait(300);
    expect(backend.calls.filter((c) => c.command === 'set_status').map((c) => c.args)).toEqual([{ id: 202, status: 'in_review', note: null }]);
    expect(app.toasts.list.at(-1)!.text).toBe('Moved 202 to In review');
    await wait(100);
    expect(app.needs.items.some((n) => n.key === item.key)).toBe(false);
  });

  it('Snooze hides it for 3 days, with Undo', async () => {
    const { backend, item, screen } = await open();
    buttons(screen.container)[3]!.click();
    await wait(300);
    expect(backend.calls.filter((c) => c.command === 'needs_snooze').map((c) => c.args)).toEqual([{ key: item.key, until: null }]);
  });

  it('↩ opens the thread, Space marks it done, ⌘⌫ dismisses', async () => {
    let { app, backend } = await open();
    app.runCommand('needs_main');
    await wait(50);
    expect(backend.calls.filter((c) => c.command === 'open_thread').map((c) => c.args)).toEqual([{ key: { source: 't3', ref: 'refund-service' } }]);

    ({ app, backend } = await open());
    app.runCommand('needs_done');
    await wait(300);
    expect(backend.calls.filter((c) => c.command === 'set_status').map((c) => c.args)).toEqual([{ id: 202, status: 'done', note: null }]);

    let item;
    ({ app, backend, item } = await open());
    app.runCommand('needs_dismiss');
    await wait(300);
    expect(backend.calls.filter((c) => c.command === 'needs_dismiss').map((c) => c.args)).toEqual([{ key: item.key }]);
  });

  it('Space does nothing on a kind without Mark done', async () => {
    const { app, backend } = await startApp('stopped');
    app.go('needs');
    app.selectNeed(app.needs.visible.find((n) => n.kind === 'question')!.key);
    app.runCommand('needs_done');
    await wait(100);
    expect(backend.calls.filter((c) => c.command === 'set_status')).toEqual([]);
  });
});
