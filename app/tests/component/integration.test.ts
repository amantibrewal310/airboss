import { describe, expect, it } from 'vitest';
import { startApp, wait } from './setup';
import type { ToastEvent } from '../../src/lib/types';

/** What the real backend sends that the UI must handle exactly (coordinator, after the merge). */
describe('Rust-sent toasts', () => {
  it('run each action as api[command](...Object.values(args)), with the warning tone', async () => {
    const { app, backend } = await startApp('design');
    const calls: [string, unknown][] = [];
    const invoke = backend.invoke.bind(backend);
    backend.invoke = (async (command: string, args: unknown) => {
      calls.push([command, args]);
      return command === 'bootstrap' ? invoke(command as never, args as never) : null;
    }) as typeof backend.invoke;
    const t: ToastEvent = {
      text: "Couldn't start 144: T3 isn't running.",
      tone: 'warning',
      task: 144,
      undo: null,
      actions: [
        { label: 'Try again', command: 'do_it', args: { id: 144, place: null, again: true } },
        { label: 'Plan it', command: 'plan_it', args: { id: 144, again: false } },
        { label: 'Open thread', command: 'open_thread', args: { key: 't3:slot-picker' } },
        { label: 'Add to T3', command: 'add_to_t3', args: { repo: 2 } },
        { label: 'Reopen and start', command: 'reopen_and_start', args: { id: 144 } },
      ],
    };
    (backend as unknown as { emit: (e: string, p: unknown) => void }).emit('toast', t);
    await wait(10);
    const toast = app.toasts.list.at(-1)!;
    expect(toast.text).toBe(t.text);
    expect(toast.tone).toBe('warn');
    expect(toast.actions.map((a) => a.label)).toEqual(t.actions.map((a) => a.label));
    for (const a of toast.actions) a.run();
    await wait(10);
    expect(calls).toEqual(t.actions.map((a) => [a.command, a.args]));
  });
});

describe('a reject Rust couldn’t write', () => {
  it('offers Retry, which rejects again with its own undo window and hides the row meanwhile', async () => {
    const { app, backend } = await startApp('design');
    const item = app.needs.visible.find((n) => n.kind === 'proposed')!;
    const [id] = item.proposed[0]!;
    const t: ToastEvent = {
      text: `Couldn't reject ${id}: the database stayed busy.`,
      tone: 'warning',
      task: id,
      undo: null,
      actions: [{ label: 'Retry', command: 'reject_proposals', args: { ids: [id], reason: null } }],
    };
    (backend as unknown as { emit: (e: string, p: unknown) => void }).emit('toast', t);
    await wait(10);
    const toast = app.toasts.list.at(-1)!;
    expect(toast.tone).toBe('warn');
    toast.actions[0]!.run();
    await wait(30);
    expect(backend.calls.filter((c) => c.command === 'reject_proposals')).toHaveLength(1);
    expect(app.tasks.hidden.has(id)).toBe(true);
    expect(app.toasts.list.at(-1)!.actions.map((a) => a.label)).toEqual(['Undo']);
  });
});

describe('quickadd:reset', () => {
  it('keeps the typed title and notes when keep_text is set, and clears them otherwise', async () => {
    const { render } = await import('vitest-browser-svelte');
    const { default: QuickAdd } = await import('../../src/windows/quickadd/QuickAdd.svelte');
    const { backend } = await startApp('design');
    const screen = await render(QuickAdd, { native: false });
    await wait(30);
    const title = screen.container.querySelector<HTMLInputElement>('input[aria-label="Title"]')!;
    const notes = screen.container.querySelector<HTMLTextAreaElement>('textarea')!;
    title.value = 'Half-typed idea';
    title.dispatchEvent(new Event('input', { bubbles: true }));
    notes.value = 'Some notes';
    notes.dispatchEvent(new Event('input', { bubbles: true }));
    title.blur();
    const emit = (p: unknown) => (backend as unknown as { emit: (e: string, p: unknown) => void }).emit('quickadd:reset', p);

    emit({ hotkey: '⌃⌥N', keep_text: true });
    await wait(30);
    expect(title.value).toBe('Half-typed idea');
    expect(notes.value).toBe('Some notes');
    expect(document.activeElement).toBe(title);

    emit({ hotkey: '⌃⌥N', keep_text: false });
    await wait(30);
    expect(title.value).toBe('');
    expect(notes.value).toBe('');
    expect(document.activeElement).toBe(title);
  });
});

describe('a link whose app Airboss couldn’t verify', () => {
  it('says so, and offers to copy the link', async () => {
    const { app, backend } = await startApp('design');
    const url = 't3code://threads/env-local/slot-picker';
    const invoke = backend.invoke.bind(backend);
    const copied: unknown[] = [];
    backend.invoke = (async (command: string, args: unknown) => {
      if (command === 'open_thread') {
        throw { code: 'app_unverified', message: "Couldn't verify the app that opens T3 links. Open the thread from T3 Code.", detail: url };
      }
      if (command === 'copy_text') copied.push(args);
      return invoke(command as never, args as never);
    }) as typeof backend.invoke;
    await app.openThread({ source: 't3', ref: 'slot-picker' });
    await wait(10);
    const toast = app.toasts.list.at(-1)!;
    expect(toast.tone).toBe('warn');
    expect(toast.text).toBe("Couldn't verify the app that opens T3 links. Open the thread from T3 Code.");
    expect(toast.actions.map((a) => a.label)).toEqual(['Copy link']);
    toast.actions[0]!.run();
    await wait(10);
    expect(copied).toEqual([{ what: { link: url } }]);
  });
});
