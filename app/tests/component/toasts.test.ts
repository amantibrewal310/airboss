import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Harness from './Harness.svelte';
import Toasts from '../../src/windows/main/Toasts.svelte';
import { startApp, wait } from './setup';

/** An error whose fix is its action: F6 reaches it, Esc closes it and puts focus back. */
describe('Toasts', () => {
  async function setup() {
    const { app } = await startApp();
    await render(Harness, { app, component: Toasts });
    const before = document.createElement('button');
    before.textContent = 'where focus was';
    document.body.append(before);
    before.focus();
    let ran = 0;
    app.toast("Couldn't start 1: scratch isn't a T3 project yet.", 'warn', [{ label: 'Add to T3 and retry', run: () => ran++ }]);
    await wait(20);
    return { app, before, ran: () => ran };
  }

  it('shows a close button only on an error that carries its fix', async () => {
    const { app, before } = await setup();
    app.toast('Copied 146');
    await wait(20);
    const toasts = document.querySelectorAll('[data-toast]');
    expect(toasts).toHaveLength(2);
    expect(toasts[0]!.querySelector('button[aria-label="Close"]')).not.toBeNull();
    expect(toasts[1]!.querySelector('button[aria-label="Close"]')).toBeNull();
    (toasts[0]!.querySelector('button[aria-label="Close"]') as HTMLButtonElement).click();
    await wait(20);
    expect(app.toasts.list.map((t) => t.text)).toEqual(['Copied 146']);
    before.remove();
  });

  it('F6 reaches the toast; Esc closes it and puts focus back', async () => {
    const { app, before, ran } = await setup();
    app.focusToasts();
    expect(document.activeElement?.textContent).toBe('Add to T3 and retry');
    // (The app's own Tab handler moves between buttons; WebKit's skips them.)
    document.querySelector<HTMLButtonElement>('[data-toast] button[aria-label="Close"]')!.focus();
    await userEvent.keyboard('{Escape}');
    await wait(20);
    expect(app.toasts.list).toEqual([]);
    expect(document.activeElement).toBe(before);
    expect(ran()).toBe(0);
    before.remove();
  });

  it('its action runs once, closes it, and puts focus back', async () => {
    const { app, before, ran } = await setup();
    app.focusToasts();
    await userEvent.keyboard('{Enter}');
    await wait(20);
    expect(ran()).toBe(1);
    expect(app.toasts.list).toEqual([]);
    expect(document.activeElement).toBe(before);
    before.remove();
  });
});
