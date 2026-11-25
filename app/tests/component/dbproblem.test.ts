import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import App from '../../src/windows/main/App.svelte';
import { startApp, wait } from './setup';
import type { AppError } from '../../src/lib/types';

/** R2: the database breaks while the app runs (the live WAL deleted, an I/O error). */
const LOST: AppError = {
  code: 'db_unavailable',
  message: 'Quit Airboss (⌘Q) and open it again. Changes made since the problem began may not have been saved. Troubleshooting has more.',
  detail: '/Users/me/Library/Application Support/Airboss/airboss.db: The write-ahead log (airboss.db-wal) was deleted while Airboss had the database open.',
};

const emit = (backend: unknown, name: string, payload: unknown) => (backend as { emit: (e: string, p: unknown) => void }).emit(name, payload);

describe('the database problem state', () => {
  it('replaces the window when db:unavailable arrives, saying what happened and what to do', async () => {
    const { app, backend } = await startApp('design');
    const screen = await render(App, { app });
    await wait(50);
    expect(screen.container.querySelector('[role="alert"]')).toBeNull();
    emit(backend, 'db:unavailable', LOST);
    await wait(30);
    const alert = screen.container.querySelector<HTMLElement>('[role="alert"]')!;
    expect(alert).toBeTruthy();
    expect(alert.querySelector('h1')!.textContent).toBe("Airboss's database stopped working");
    expect(alert.querySelector('code')!.textContent).toBe(LOST.detail);
    expect(alert.querySelector('p')!.textContent).toBe(LOST.message);
    const buttons = [...alert.querySelectorAll('button')].map((b) => b.textContent!.trim());
    // Trying again can't help: the app stays in this state until it's quit.
    expect(buttons).toEqual(['Show in Finder']);
  });

  it('Show in Finder opens the database’s folder, even when SQLite’s reason has a slash', async () => {
    const { app, backend } = await startApp('design');
    const screen = await render(App, { app });
    await wait(30);
    emit(backend, 'db:unavailable', { ...LOST, detail: '/Users/me/Library/Application Support/Airboss/airboss.db: disk I/O error' });
    await wait(30);
    screen.container.querySelector<HTMLButtonElement>('[role="alert"] button')!.click();
    await wait(10);
    expect(backend.calls.filter((c) => c.command === 'open_url').map((c) => c.args)).toEqual([
      { target: { url: 'file:///Users/me/Library/Application Support/Airboss' } },
    ]);
  });

  it('a command answered with db_unavailable shows it too, instead of a toast', async () => {
    const { app, backend } = await startApp('design');
    backend.invoke = (async () => {
      throw LOST;
    }) as typeof backend.invoke;
    await app.toggleDone(144);
    await wait(10);
    expect(app.fatal).toEqual(LOST);
    expect(app.fatalLost).toBe(true);
    expect(app.toasts.list.filter((t) => t.text === LOST.message)).toEqual([]);
  });

  it('at start it stays “can’t open”, with Try again', async () => {
    const { app } = await startApp('dberror');
    const screen = await render(App, { app });
    await wait(30);
    const alert = screen.container.querySelector<HTMLElement>('[role="alert"]')!;
    expect(alert.querySelector('h1')!.textContent).toBe("Airboss can't open its database");
    expect([...alert.querySelectorAll('button')].map((b) => b.textContent!.trim())).toEqual(['Show in Finder', 'Try again']);
    expect(app.fatalLost).toBe(false);
  });
});
