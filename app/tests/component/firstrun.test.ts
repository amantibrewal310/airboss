import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Harness from './Harness.svelte';
import FirstRun from '../../src/windows/main/FirstRun.svelte';
import AddRepos from '../../src/windows/main/AddRepos.svelte';
import Settings from '../../src/windows/main/overlays/Settings.svelte';
import { startApp, wait } from './setup';

const agentsRow = () => [...document.querySelectorAll<HTMLElement>('.c')].find((c) => c.textContent!.includes('Agents ·'))!;
const agentsButton = () => agentsRow().querySelector<HTMLButtonElement>('button')!;
const checkboxes = () => [...document.querySelectorAll<HTMLElement>('[role="checkbox"]')];

/** #9: installing the CLI isn't enough; Ready needs an agent tool that has the server. */
describe('First run › Agents', () => {
  it('goes Not set up → Not connected → Ready, on one button that keeps focus', async () => {
    const { app, backend } = await startApp('welcome');
    await render(Harness, { app, component: FirstRun });
    await wait(30);
    expect(agentsRow().textContent).toContain('Agents · Not set up');
    expect(agentsButton().textContent).toBe('Install command-line tool');

    agentsButton().focus();
    agentsButton().click();
    await wait(30);
    expect(agentsRow().textContent).toContain('Agents · Not connected');
    expect(agentsRow().textContent).toContain("Agents can't report to Airboss yet.");
    expect(agentsButton().textContent).toBe('Connect Claude Code');
    expect(app.conn.footer).toEqual({ ok: false, text: 'Agents not connected' });

    agentsButton().click();
    await wait(60);
    expect(backend.s.agents.claude).toBe(true);
    expect(agentsRow().textContent).toContain('Agents · Ready');
    expect(agentsRow().textContent).toContain('No agent has reported yet.');
    expect(agentsButton().textContent).toBe('Copy agent note');
    expect(document.activeElement).toBe(agentsButton());
    expect(app.toasts.list.at(-1)!.text).toBe('Connected Claude Code');
  });

  it('says why Connect failed and offers the setup command instead', async () => {
    const { app, backend } = await startApp('welcome');
    backend.s.connectClaudeError = "Couldn't connect Claude Code: Error: Invalid scope: user";
    await render(Harness, { app, component: FirstRun });
    await wait(30);
    agentsButton().click();
    await wait(30);
    agentsButton().click();
    await wait(60);
    const toast = app.toasts.list.at(-1)!;
    expect(toast).toMatchObject({ text: "Couldn't connect Claude Code: Error: Invalid scope: user", tone: 'warn' });
    expect(toast.actions.map((a) => a.label)).toEqual(['Copy setup command']);
    expect(agentsRow().textContent).toContain('Agents · Not connected');
    toast.actions[0]!.run();
    await wait(20);
    expect(backend.opened.at(-1)).toBe('clipboard:claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp');
  });
});

/** #3: adding one folder keeps the Repos step, its ticks, and focus. */
describe('First run › Repos', () => {
  it('stays after Add another folder…, keeps the ticks, lists the folder as added, and keeps focus', async () => {
    const { app } = await startApp('welcome');
    await render(Harness, { app, component: FirstRun });
    await wait(40);
    checkboxes()[1]!.click();
    const another = [...document.querySelectorAll('button')].find((b) => b.textContent!.includes('Add another folder'))!;
    another.click();
    await wait(60);
    expect(app.tasks.repos.map((r) => r.name)).toEqual(['side-project']);
    expect([...document.querySelectorAll('h2')].map((h) => h.textContent)).toContain('Repos');
    const rows = checkboxes();
    expect(rows[1]!.getAttribute('aria-checked')).toBe('true');
    const added = rows.find((r) => r.getAttribute('aria-label') === 'side-project')!;
    expect(added.getAttribute('aria-checked')).toBe('true');
    expect(added.getAttribute('aria-disabled')).toBe('true');
    expect(added.textContent).toContain('Added');
    expect(document.activeElement).toBe(another);

    // Adding the ticked one shows it as added too, and focus stays in the list.
    const addBtn = [...document.querySelectorAll('button')].find((b) => /^Add 1 repo/.test(b.textContent!.trim()))!;
    addBtn.click();
    await wait(60);
    expect(app.tasks.repos.map((r) => r.name)).toEqual(['side-project', 'booking-web']);
    expect(checkboxes()[1]!.getAttribute('aria-disabled')).toBe('true');
    expect(checkboxes()[1]!.getAttribute('aria-checked')).toBe('true');
    expect(document.activeElement?.getAttribute('role')).toBe('checkbox');
  });
});

/** #2: the project list is one Tab stop. */
describe('AddRepos › keyboard', () => {
  it('has one Tab stop; ↑↓, Home and End move it; rows are named by the project, described by the path', async () => {
    const { app } = await startApp('welcome');
    await render(Harness, { app, component: AddRepos });
    await wait(30);
    const stops = () => checkboxes().filter((r) => r.tabIndex === 0);
    expect(stops()).toHaveLength(1);
    expect(stops()[0]).toBe(checkboxes()[0]);
    const first = checkboxes()[0]!;
    expect(first.getAttribute('aria-label')).toBe('airboss');
    const desc = first.getAttribute('aria-describedby')!.split(' ').map((id) => document.getElementById(id)!.textContent);
    expect(desc).toEqual(['~/Code/airboss']);
    const notes = checkboxes()[3]!;
    expect(notes.getAttribute('aria-describedby')!.split(' ').map((id) => document.getElementById(id)!.textContent)).toEqual([
      '~/Documents/notes',
      'Not a git repository',
    ]);

    first.focus();
    await userEvent.keyboard('{End}');
    expect(document.activeElement).toBe(checkboxes()[4]);
    expect(stops()).toEqual([checkboxes()[4]]);
    await userEvent.keyboard('{ArrowUp}{ArrowUp}');
    expect(document.activeElement).toBe(checkboxes()[2]);
    await userEvent.keyboard(' ');
    expect(checkboxes()[2]!.getAttribute('aria-checked')).toBe('true');
    await userEvent.keyboard('{Home}');
    expect(document.activeElement).toBe(checkboxes()[0]);
    // Tab leaves the list in one press.
    await userEvent.keyboard('{Tab}');
    expect(document.activeElement?.getAttribute('role')).not.toBe('checkbox');
    expect(document.activeElement?.textContent).toContain('Add another folder');
  });
});

describe('Settings › Agent setup', () => {
  it('shows each agent tool as connected, or how to connect it', async () => {
    const { app, backend } = await startApp('design');
    await render(Harness, { app, component: Settings });
    await wait(30);
    const line = (name: string) => [...document.querySelectorAll<HTMLElement>('.tools li')].find((l) => l.textContent!.includes(name))!;
    expect(line('Claude Code').textContent).toContain('Connected');
    expect(line('Claude Code').querySelector('button')).toBeNull();
    expect(line('Codex').querySelector('button')!.textContent).toBe('Copy Codex setup');
    line('Codex').querySelector('button')!.click();
    await wait(20);
    expect(backend.opened.at(-1)).toContain('env_vars = ["T3CODE_THREAD_ID"]');
    expect(app.toasts.list.at(-1)!.text).toContain('~/.codex/config.toml');
    const texts = [...document.querySelectorAll('button')].map((b) => b.textContent!.trim());
    expect(texts).toContain('Copy agent note');
    expect(texts).toContain('Copy setup command');
  });

  it('connects Claude Code from its line', async () => {
    const { app } = await startApp('welcome');
    await app.installCli();
    await render(Harness, { app, component: Settings });
    await wait(30);
    const claude = () => [...document.querySelectorAll<HTMLElement>('.tools li')].find((l) => l.textContent!.includes('Claude Code'))!;
    expect(claude().textContent).toContain('Not connected');
    claude().querySelector('button')!.click();
    await wait(60);
    expect(claude().textContent).toContain('Connected');
    expect(app.conn.merged.find((c) => c.name === 'Agents')).toMatchObject({ ok: true, state: 'Ready' });
  });
});
