import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Harness from './Harness.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import { startApp, wait } from './setup';
import { tasksLinkedTo } from '../../src/lib/logic/repos';

const q = <T extends HTMLElement>(sel: string) => document.querySelector<T>(sel);

/** Settings › Repos (final audit should-fix 8). */
describe('Settings › Repos', () => {
  it('renames a repo with ↩, keeping its folder', async () => {
    const { app, backend } = await startApp('design');
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(30);
    const repo = app.tasks.repos[0]!;
    q<HTMLButtonElement>(`[data-edit="${repo.id}"]`)!.click();
    await wait(30);
    const input = q<HTMLInputElement>(`#repo-name-${repo.id}`)!;
    expect(document.activeElement).toBe(input);
    input.select();
    await userEvent.keyboard('booking-web-2{Enter}');
    await wait(30);
    const call = backend.calls.find((c) => c.command === 'edit_repo')!;
    expect(call.args).toEqual({ repo: repo.id, name: 'booking-web-2', path: repo.path, remote: repo.remote_url });
    expect(app.tasks.repos.find((r) => r.id === repo.id)!.name).toBe('booking-web-2');
    expect(app.toasts.list.at(-1)!.text).toBe('Saved booking-web-2');
    // Back on the row's Edit button, and Settings is still open.
    expect(document.activeElement).toBe(q(`[data-edit="${repo.id}"]`));
    expect(app.ui.overlay?.kind).toBe('settings');
  });

  it('shows a refused edit inline', async () => {
    const { app } = await startApp('design');
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(30);
    const [a, b] = app.tasks.repos;
    q<HTMLButtonElement>(`[data-edit="${a!.id}"]`)!.click();
    await wait(30);
    q<HTMLInputElement>(`#repo-name-${a!.id}`)!.select();
    await userEvent.keyboard(`${b!.name}{Enter}`);
    await wait(30);
    expect(q('.form [role="alert"]')!.textContent).toBe('A repo with that name or folder is already added.');
  });

  it('asks before removing, with how many tasks lose the link; Esc cancels without closing Settings', async () => {
    const { app, backend } = await startApp('design');
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(30);
    const repo = app.tasks.repos[0]!;
    const linked = tasksLinkedTo(app.tasks.rows, repo.id);
    expect(linked).toBeGreaterThan(1);
    q<HTMLButtonElement>(`[data-remove="${repo.id}"]`)!.click();
    await wait(30);
    expect(q('[role="alertdialog"] p')!.textContent).toBe(`Remove ${repo.name}? ${linked} tasks lose the link.`);
    expect(document.activeElement).toBe(q(`[data-cancel-remove="${repo.id}"]`));
    await userEvent.keyboard('{Escape}');
    await wait(30);
    expect(q('[role="alertdialog"]')).toBeNull();
    expect(app.ui.overlay?.kind).toBe('settings');
    expect(backend.calls.some((c) => c.command === 'remove_repo')).toBe(false);

    q<HTMLButtonElement>(`[data-remove="${repo.id}"]`)!.click();
    await wait(30);
    [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find((b) => b.textContent!.trim() === 'Remove')!.click();
    await wait(30);
    expect(backend.calls.filter((c) => c.command === 'remove_repo').map((c) => c.args)).toEqual([{ repo: repo.id }]);
    expect(app.tasks.repos.some((r) => r.id === repo.id)).toBe(false);
    expect(app.toasts.list.at(-1)!.text).toBe(`Removed ${repo.name}. ${linked} tasks lost the link.`);
  });

  it('backs up the database through a save dialog', async () => {
    const { app, backend } = await startApp('design');
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(30);
    [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent!.trim() === 'Back up…')!.click();
    await wait(30);
    expect(backend.calls.some((c) => c.command === 'back_up_database')).toBe(true);
    expect(app.toasts.list.at(-1)!.text).toBe('Backed up to ~/Documents/Airboss backup.db');
  });
});

describe('Settings › Notifications', () => {
  it('is on by default and turns off and on', async () => {
    const { app, backend } = await startApp('design');
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(30);
    const toggle = q<HTMLButtonElement>('[role="switch"][aria-label="Notifications"]')!;
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    toggle.click();
    await wait(30);
    expect(app.settings.notifications).toBe(false);
    const saved = backend.calls.filter((c) => c.command === 'set_settings').at(-1)!.args as { settings: { notifications: boolean } };
    expect(saved.settings.notifications).toBe(false);
    expect(q('.blocked')).toBeNull();
  });

  it('explains how to allow them when macOS has them off, with a way there', async () => {
    const { app, backend } = await startApp('design');
    (backend as unknown as { s: { notificationPermission: string } }).s.notificationPermission = 'denied';
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(50);
    const note = q<HTMLElement>('.blocked')!;
    expect(note.textContent).toContain('System Settings › Notifications › Airboss');
    note.querySelector<HTMLButtonElement>('button')!.click();
    await wait(30);
    expect(backend.calls.some((c) => c.command === 'open_notification_settings')).toBe(true);
  });
});
