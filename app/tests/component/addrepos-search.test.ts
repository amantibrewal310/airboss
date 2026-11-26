import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import { startApp, wait } from './setup';

/** Add repos' search (owner's request, 26 Nov 2026: "Search option"). */

/** Types into the field as the browser would report it. */
async function type(input: HTMLInputElement, text: string) {
  input.value = text;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(10);
}

/** A key on whatever has focus, bubbling as a real key press does. */
async function press(key: string) {
  const at = (document.activeElement ?? document.body) as HTMLElement;
  at.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  await wait(10);
}
async function openWith(extra: number) {
  const { app, backend } = await startApp('design');
  for (let i = 0; i < extra; i++) {
    const path = `~/Workspace/Work/Projects/project-${i}`;
    backend.s.t3Projects.push({ id: `p${i}`, name: `project-${i}`, path });
    backend.s.gitFolders.push(path);
  }
  backend.s.t3Projects.push({ id: 'goose', name: 'goose', path: '~/Workspace/Work/Projects/goose' });
  backend.s.gitFolders.push('~/Workspace/Work/Projects/goose');
  await render(Harness, { app, component: Overlays });
  app.open({ kind: 'add_repos' });
  await wait(60);
  const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
  const search = dialog.querySelector<HTMLInputElement>('input[type="search"]');
  const rows = () => [...dialog.querySelectorAll<HTMLElement>('[role="checkbox"]')].map((r) => r.getAttribute('aria-label'));
  const status = () => dialog.querySelector('[role="status"]')!.textContent;
  return { app, dialog, search, rows, status };
}

describe("Add repos' search", () => {
  it('only shows on a long list', async () => {
    const { search, rows } = await openWith(0);
    expect(rows()).toHaveLength(6);
    expect(search).toBeNull();
  });

  it('opens on the search field and filters by name or path as you type', async () => {
    const { search, rows, status } = await openWith(10);
    expect(document.activeElement).toBe(search);
    expect(search!.placeholder).toBe('Search 16 repos');
    await type(search!, 'goose');
    expect(rows()).toEqual(['goose']);
    expect(status()).toBe('1 of 16 repos');
    await type(search!, 'project 1');
    expect(rows()).toEqual(['project-1']);
    await type(search!, 'Code');
    // The design fixture's own repos live in ~/Code; only path matches.
    expect(rows()).toEqual(['airboss', 'booking-web', 'booking-api', 'personal-site']);
  });

  it('keeps ticks while filtering, and adds them all', async () => {
    const { app, dialog, search, rows } = await openWith(10);
    await type(search!, 'project-3');
    (dialog.querySelector('[role="checkbox"]') as HTMLElement).click();
    await type(search!, 'goose');
    (dialog.querySelector('[role="checkbox"]') as HTMLElement).click();
    await type(search!, '');
    expect(rows()).toHaveLength(16);
    const add = [...dialog.querySelectorAll('button')].find((b) => b.textContent!.trim() === 'Add 2 repos')!;
    expect(add).toBeTruthy();
    add.click();
    await wait(60);
    expect(app.tasks.repos.map((r) => r.name)).toEqual(expect.arrayContaining(['project-3', 'goose']));
  });

  it('says when nothing matches, and Esc clears the search before it closes', async () => {
    const { app, dialog, search, rows } = await openWith(10);
    await type(search!, 'zzz');
    expect(rows()).toEqual([]);
    expect(dialog.textContent).toContain('No repo matches “zzz”. Not there? Use Add another folder… below.');
    await press('Escape');
    expect(search!.value).toBe('');
    expect(rows()).toHaveLength(16);
    expect(app.ui.overlay?.kind).toBe('add_repos');
    await press('Escape');
    expect(app.ui.overlay).toBeNull();
  });

  it('↓ goes from the field into the list, and ↑ from the first row comes back', async () => {
    const { dialog, search } = await openWith(10);
    await type(search!, 'project');
    await press('ArrowDown');
    await wait(10);
    const first = dialog.querySelector('[role="checkbox"]')!;
    expect(document.activeElement).toBe(first);
    expect(first.getAttribute('aria-label')).toBe('project-0');
    await press('ArrowUp');
    expect(document.activeElement).toBe(search);
  });
});
