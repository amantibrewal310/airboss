import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Menu from '../../src/lib/components/Menu.svelte';
import Harness from './Harness.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import { startApp, wait } from './setup';

const count = (calls: { command: string }[], name: string) => calls.filter((c) => c.command === name).length;

describe('P0-1: a quick second main action in Needs me', () => {
  it('doesn’t run the next item’s action 250 ms later', async () => {
    const { app, backend } = await startApp();
    // Select the Ready item; the next one after it is Finish it?.
    app.selectNeed(app.needs.visible.find((n) => n.kind === 'ready')!.key);
    app.runCommand('needs_main');
    await wait(250);
    app.runCommand('needs_main');
    await wait(300);
    expect(count(backend.calls, 'do_it')).toBe(1);
    expect(count(backend.calls, 'set_status')).toBe(0);
  });

  it('doesn’t open the same thread twice 120 ms later', async () => {
    const { app, backend } = await startApp();
    app.runCommand('needs_main');
    await wait(120);
    app.runCommand('needs_main');
    await wait(300);
    expect(count(backend.calls, 'open_thread')).toBe(1);
  });

  it('acts again once the user moves the selection', async () => {
    const { app, backend } = await startApp();
    app.runCommand('needs_main');
    await wait(50);
    app.moveNeed(1);
    app.runCommand('needs_main');
    await wait(50);
    expect(count(backend.calls, 'open_thread')).toBe(2);
  });
});

describe('P0-2: a menu opened by a key press', () => {
  const items = (picked: string[]) => [
    { id: 'a', label: 'Start in airboss', run: () => picked.push('a') },
    { id: 'b', label: 'Start in booking-web', run: () => picked.push('b') },
  ];

  it('ignores ⌘↩', async () => {
    const picked: string[] = [];
    await render(Menu, { items: items(picked), x: 100, y: 100, label: 'Start in', onclose: () => {} });
    await wait(350);
    await userEvent.keyboard('{Meta>}{Enter}{/Meta}');
    expect(picked).toEqual([]);
  });

  it('ignores ↩ within 300 ms of opening, then picks', async () => {
    const picked: string[] = [];
    await render(Menu, { items: items(picked), x: 100, y: 100, label: 'Start in', onclose: () => {} });
    await userEvent.keyboard('{Enter}');
    expect(picked).toEqual([]);
    await wait(320);
    await userEvent.keyboard('{Enter}');
    expect(picked).toEqual(['a']);
  });

  it('New task ⌘↩ twice doesn’t pick “Start in” for you', async () => {
    const { app, backend } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'new', parent: null, repos: [], plan: false });
    await wait(30);
    await userEvent.keyboard('Research queues{Meta>}{Enter}{/Meta}');
    await wait(80);
    await userEvent.keyboard('{Meta>}{Enter}{/Meta}');
    await wait(80);
    const doIts = backend.calls.filter((c) => c.command === 'do_it');
    expect(doIts.every((c) => (c.args as { place: unknown }).place === null)).toBe(true);
    expect(app.ui.overlay?.kind).toBe('start_in');
  });
});

describe('P1-14: ↩↩ in New task', () => {
  it('creates the task without opening rename on it', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.go('all');
    app.open({ kind: 'new', parent: null, repos: [], plan: false });
    await wait(30);
    await userEvent.keyboard('Fix double booking{Enter}');
    await wait(60);
    app.handleKey(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', cancelable: true }));
    expect(app.ui.renaming).toBeNull();
  });
});

describe('P0-4: ⌘Z while a reject is pending', () => {
  it('undoes the latest write without bringing the rejected rows back', async () => {
    const { app, backend } = await startApp();
    const item = app.needs.visible.find((n) => n.kind === 'proposed')!;
    await app.rejectProposals(item, [153]);
    await app.toggleDone(146);
    await app.undo();
    expect(app.tasks.rows.get(146)!.status).toBe('todo');
    expect(app.tasks.hidden.has(153)).toBe(true);
    await wait(250);
    expect(backend.s.tasks.get(153)!.status).toBe('dropped');
  });
});

describe('P0-5: the reject toast after the reject is written', () => {
  it('drops Undo at the deadline even while hovered', async () => {
    const { app } = await startApp();
    // Rust writes a reject 5 s after it's made (SPEC-app); the test shortens that.
    const { App } = await import('../../src/lib/state/app.svelte');
    App.rejectDelayMs = 200;
    const item = app.needs.visible.find((n) => n.kind === 'proposed')!;
    await app.rejectProposals(item, [153]);
    App.rejectDelayMs = 5000;
    const toast = app.toasts.list.at(-1)!;
    expect(toast.actions.map((a) => a.label)).toEqual(['Undo']);
    app.toasts.pause(toast.id);
    await wait(250);
    const now = app.toasts.list.find((t) => t.id === toast.id);
    expect(now?.actions ?? []).toEqual([]);
  });
});

describe('P2-23: Undo in Ready', () => {
  it('brings the row back and selects it', async () => {
    const { app } = await startApp();
    app.go('ready');
    app.select(146);
    await app.toggleDone(146);
    expect(app.ui.selected).not.toBe(146);
    await app.undo();
    expect(app.visibleIds).toContain(146);
    expect(app.ui.selected).toBe(146);
  });
});
