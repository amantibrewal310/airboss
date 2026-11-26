import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import FirstRun from '../../src/windows/main/FirstRun.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import TaskPanel from '../../src/windows/main/TaskPanel.svelte';
import { startApp, wait } from './setup';

/** SPEC-herdr "app": Herdr as the second place agents start, from the window's side. */

const checkboxes = () => [...document.querySelectorAll<HTMLElement>('[role="checkbox"]')];
const connRow = (name: string) => [...document.querySelectorAll<HTMLElement>('.c')].find((c) => c.textContent!.includes(`${name} ·`));
const radio = (group: string, label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>(`[role="radiogroup"][aria-label="${group}"] [role="radio"]`)].find((b) => b.textContent === label)!;

describe('First run without T3 Code', () => {
  it("lists Herdr's folders under From Herdr, with the same rules as T3 projects", async () => {
    const { app, backend } = await startApp('herdr');
    await render(Harness, { app, component: FirstRun });
    await wait(40);
    expect(backend.calls.some((c) => c.command === 'herdr_folders')).toBe(true);
    expect(backend.calls.some((c) => c.command === 't3_projects')).toBe(false);
    const heading = [...document.querySelectorAll('h3')].find((h) => h.textContent === 'From Herdr')!;
    expect(heading).toBeTruthy();
    const list = document.querySelector('[role="group"]')!;
    expect(list.getAttribute('aria-labelledby')).toBe(heading.id);
    const rows = checkboxes();
    expect(rows.map((r) => r.getAttribute('aria-label'))).toEqual(['booking-web', 'Notes', 'scratch', 'personal-site']);
    // Folders that aren't git repos are greyed with the reason.
    const notes = rows[1]!;
    expect(notes.getAttribute('aria-disabled')).toBe('true');
    expect(notes.textContent).toContain('Not a git repository');
    // Tick one, add it: it shows as Added.
    rows[0]!.click();
    await wait(20);
    const add = [...document.querySelectorAll('button')].find((b) => b.textContent!.trim().startsWith('Add 1 repo'))!;
    add.click();
    await wait(60);
    expect(app.tasks.repos.map((r) => r.name)).toEqual(['booking-web']);
    const added = checkboxes().find((r) => r.getAttribute('aria-label') === 'booking-web')!;
    expect(added.textContent).toContain('Added');
    expect(added.getAttribute('aria-disabled')).toBe('true');
  });

  it('shows Herdr as Ready and no T3 Code row, and starts agents in Herdr', async () => {
    const { app } = await startApp('herdr');
    await render(Harness, { app, component: FirstRun });
    await wait(30);
    expect(connRow('Herdr')!.textContent).toContain('Herdr · Ready');
    expect(connRow('Herdr')!.textContent).toContain('0.1.18');
    expect(connRow('T3 Code')).toBeUndefined();
    expect(app.conn.launcher).toBe('herdr');
    expect(app.conn.launchReady).toBe(true);
    expect(app.conn.footer.text).toBe('All connected');
  });

  it('keeps T3 projects when T3 Code is there', async () => {
    const { app, backend } = await startApp('welcome');
    await render(Harness, { app, component: FirstRun });
    await wait(40);
    expect(backend.calls.some((c) => c.command === 't3_projects')).toBe(true);
    expect(backend.calls.some((c) => c.command === 'herdr_folders')).toBe(false);
    expect([...document.querySelectorAll('h3')].some((h) => h.textContent === 'From Herdr')).toBe(false);
  });
});

describe('Settings › Agent setup › Start agents in', () => {
  it('saves the choice, and Do it then names Herdr when it is missing', async () => {
    const { app, backend } = await startApp('design');
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'settings' });
    await wait(30);
    expect(radio('Start agents in', 'Automatic').getAttribute('aria-checked')).toBe('true');
    // Herdr isn't installed or chosen yet: no agent choice.
    expect(document.querySelector('[aria-label="Agent in Herdr"]')).toBeNull();
    radio('Start agents in', 'Herdr').click();
    await wait(30);
    expect(backend.calls.find((c) => c.command === 'set_settings')!.args).toMatchObject({ settings: { start_agents_in: 'herdr', herdr_agent: 'claude' } });
    expect(app.settings.start_agents_in).toBe('herdr');
    expect(app.conn.launcher).toBe('herdr');
    expect(app.conn.launchReady).toBe(false);
    expect(app.conn.launchOff).toBe("Herdr isn't installed");
    // Now chosen, the agent choice shows.
    radio('Agent in Herdr', 'Codex').click();
    await wait(30);
    expect(app.settings.herdr_agent).toBe('codex');
    // The Herdr row says it's missing.
    expect(app.conn.merged.find((c) => c.name === 'Herdr')).toMatchObject({ ok: false, state: 'Not installed' });
    // Do it refuses, naming Herdr.
    await app.doIt(144);
    expect(app.toasts.list.at(-1)!.text).toBe("Herdr isn't installed.");
    expect(backend.calls.some((c) => c.command === 'do_it')).toBe(false);
  });
});

describe('Do it in Herdr', () => {
  it('disables the buttons with the reason that names Herdr, and the line under the header too', async () => {
    const { app } = await startApp('design');
    await app.saveSettings({ ...app.settings, start_agents_in: 'herdr' });
    app.go('all');
    app.select(146, { open: true });
    await render(Harness, { app, component: TaskPanel });
    await wait(30);
    expect(document.querySelector('[role="status"]')!.textContent).toContain('Do it and Plan it need Herdr. Install it, then choose Check again.');
    const btn = document.querySelector<HTMLButtonElement>('[data-do-it]')!;
    expect(btn.getAttribute('aria-disabled')).toBe('true');
    expect(btn.title).toBe("Herdr isn't installed");
  });

  it('starts a Herdr terminal, links it, and Open thread shows it in Herdr', async () => {
    const { app, backend } = await startApp('design');
    backend.s.herdr = { ready: { version: '0.1.18' } };
    await app.saveSettings({ ...app.settings, start_agents_in: 'herdr' });
    await app.doIt(146);
    await wait(60);
    const toast = app.toasts.list.at(-1)!;
    expect(toast.text).toMatch(/^Started a thread for #146/);
    const link = backend.s.links.filter((l) => l.task === 146).at(-1)!;
    expect(link.kind).toBe('herdr_terminal');
    expect(link.url).toBeNull();
    // Add to T3 is never offered for Herdr.
    expect(toast.actions.map((a) => a.label)).toEqual(['Open thread']);
    toast.actions[0]!.run();
    await wait(20);
    expect(backend.opened.at(-1)).toBe(`herdr:${link.ref}`);
  });
});

describe('A Herdr thread on a task', () => {
  it('shows its chip, title and Open, and its link tag opens it in Herdr', async () => {
    const { app, backend } = await startApp('full');
    app.go('all');
    app.select(179, { open: true });
    await render(Harness, { app, component: TaskPanel });
    await wait(60);
    const threads = document.querySelector('[aria-label="Threads"]')!;
    expect(threads.textContent).toContain('179 Tmux status line');
    expect(threads.textContent).toContain('working');
    const open = [...threads.querySelectorAll('button')].find((b) => b.textContent!.includes('Open'))!;
    open.click();
    await wait(20);
    expect(backend.calls.at(-1)).toMatchObject({ command: 'open_thread', args: { key: { source: 'herdr', ref: 't4p' } } });
    expect(backend.opened.at(-1)).toBe('herdr:t4p');
    // The tag under Links opens the same terminal.
    const tag = [...document.querySelectorAll<HTMLButtonElement>('button.in')].find((b) => b.textContent === 'Herdr · 179 Tmux status line')!;
    tag.click();
    await wait(20);
    expect(backend.calls.at(-1)).toMatchObject({ command: 'open_thread', args: { key: { source: 'herdr', ref: 't4p' } } });
  });

  it("says so when Herdr couldn't show it", async () => {
    const { app, backend } = await startApp('full');
    backend.s.herdrFails = true;
    app.openThread({ source: 'herdr', ref: 't4p' });
    await wait(20);
    expect(app.toasts.list.at(-1)!.text).toBe("Couldn't show that terminal in Herdr.");
  });
});
