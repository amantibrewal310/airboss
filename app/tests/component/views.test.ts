import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Harness from './Harness.svelte';
import TaskPanel from '../../src/windows/main/TaskPanel.svelte';
import NeedsInbox from '../../src/windows/main/NeedsInbox.svelte';
import AddRepos from '../../src/windows/main/AddRepos.svelte';
import { mainLabel } from '../../src/lib/logic/needs';
import { startApp, wait } from './setup';

describe('TaskPanel', () => {
  it('renames in place: click the title, ↩ saves', async () => {
    const { app } = await startApp();
    app.go('all');
    app.select(146, { open: true });
    await render(Harness, { app, component: TaskPanel });
    await wait(20);
    document.querySelector<HTMLButtonElement>('h1 button')!.click();
    await wait(10);
    await userEvent.keyboard('{Meta>}a{/Meta}Confirmation email with .ics{Enter}');
    await wait(30);
    expect(app.tasks.rows.get(146)!.title).toBe('Confirmation email with .ics');
  });

  it('edits notes; ⌘↩ saves and Esc cancels', async () => {
    const { app } = await startApp();
    app.go('all');
    app.select(152, { open: true });
    await render(Harness, { app, component: TaskPanel });
    await wait(30);
    const notes = () => document.querySelector<HTMLElement>('.doc')!;
    expect(notes().textContent).toContain('No notes yet');
    notes().click();
    await wait(260);
    await userEvent.keyboard('Start with the sortie repo’s README.{Meta>}{Enter}{/Meta}');
    await wait(40);
    expect(app.detail.detail!.task.body).toBe('Start with the sortie repo’s README.');
    notes().click();
    await wait(260);
    await userEvent.keyboard(' More.{Escape}');
    await wait(30);
    expect(app.detail.detail!.task.body).toBe('Start with the sortie repo’s README.');
  });

  it('shows “Starting…” while Do it is in flight, and a second press does nothing', async () => {
    const { app, backend } = await startApp('design', { launch: 300, attention: 20 });
    app.go('all');
    app.select(146, { open: true });
    await render(Harness, { app, component: TaskPanel });
    await wait(20);
    const btn = document.querySelector<HTMLButtonElement>('[data-do-it]')!;
    btn.click();
    await wait(30);
    expect(btn.textContent).toContain('Starting…');
    expect(btn.getAttribute('aria-busy')).toBe('true');
    btn.click();
    await wait(400);
    expect(backend.calls.filter((c) => c.command === 'do_it')).toHaveLength(1);
    expect(btn.textContent).toContain('Do it');
    expect(app.toasts.list.at(-1)!.text).toBe('Started a thread for #146 in booking-api');
  });

  it('switches Do it and Plan it off without T3 Code, with the line under the header', async () => {
    const { app, backend } = await startApp('t3missing');
    app.go('all');
    app.select(146, { open: true });
    await render(Harness, { app, component: TaskPanel });
    await wait(20);
    expect(document.querySelector('[role="status"]')!.textContent).toContain('Starting agents needs T3 Code. Install it, then choose Check again.');
    const btn = document.querySelector<HTMLButtonElement>('[data-do-it]')!;
    expect(btn.getAttribute('aria-disabled')).toBe('true');
    btn.click();
    await wait(20);
    expect(app.toasts.list.at(-1)!.text).toContain('needs T3 Code');
    expect(backend.calls.some((c) => c.command === 'do_it')).toBe(false);
  });
});

describe('NeedsInbox', () => {
  it('shows every kind’s main action on its card', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: NeedsInbox });
    for (const n of app.needs.visible) {
      app.selectNeed(n.key);
      await wait(10);
      expect(document.querySelector('.big .pri')!.textContent).toContain(mainLabel(n.kind));
    }
  });

  it('slides a dismissed item out at once and selects the next one', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: NeedsInbox });
    const [first, second] = app.needs.visible;
    app.selectNeed(first!.key);
    await wait(10);
    void app.dismiss();
    await wait(10);
    expect(document.getElementById(`need-${first!.key}`)!.classList.contains('gone')).toBe(true);
    await wait(250);
    expect(document.getElementById(`need-${first!.key}`)).toBeNull();
    expect(app.needs.selectedKey).toBe(second!.key);
  });

  it('accepts one proposal, and edits one before accepting it', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: NeedsInbox });
    const item = app.needs.visible.find((n) => n.kind === 'proposed')!;
    app.selectNeed(item.key);
    await wait(20);
    expect(document.querySelectorAll('.prop-row')).toHaveLength(3);
    (document.querySelector('.prop-row .g') as HTMLButtonElement).click();
    await wait(30);
    expect(app.tasks.rows.get(153)!.status).toBe('todo');
    const edit = [...document.querySelectorAll<HTMLButtonElement>('.prop-row .g')].find((b) => b.textContent === 'Edit')!;
    edit.click();
    await wait(10);
    await userEvent.keyboard('{Meta>}a{/Meta}List merged worktrees{Enter}');
    await wait(40);
    expect(app.tasks.rows.get(154)!.title).toBe('List merged worktrees');
    expect(app.tasks.rows.get(154)!.status).toBe('todo');
  });

  it('says so when nothing needs you', async () => {
    const { app } = await startApp('nothing');
    await render(Harness, { app, component: NeedsInbox });
    expect(document.querySelector('h2')!.textContent).toBe('Nothing needs you');
    expect(document.body.textContent!.replace(/\s+/g, ' ')).toMatch(/Checked \d\d:\d\d · 9 in progress · \d+ ready to start/);
    expect(document.querySelector('.empty button')?.textContent).toContain('Show ready tasks');
  });

  it('offers no "Show ready tasks" when nothing is ready (hands-on QA #15)', async () => {
    const { app } = await startApp('nothing');
    await render(Harness, { app, component: NeedsInbox });
    const upserts = [...app.tasks.rows.values()].filter((r) => r.ready).map((r) => ({ ...r, ready: false }));
    app.onPatch({ seq: app.tasks.seq + 1, upserts, removed: [], order: null, repos: null, created: [] });
    await wait(10);
    expect(app.counts.ready).toBe(0);
    expect(document.body.textContent!.replace(/\s+/g, ' ')).toContain('0 ready to start');
    expect(document.querySelector('.empty button')).toBeNull();
  });
});

describe('AddRepos', () => {
  it('lists T3’s projects unticked, greys the ones it can’t add, and adds the ticked ones', async () => {
    const { app } = await startApp('welcome');
    await render(Harness, { app, component: AddRepos });
    await wait(30);
    const rows = [...document.querySelectorAll<HTMLElement>('[role="checkbox"]')];
    expect(rows.map((r) => r.querySelector('.name')!.textContent)).toEqual(['airboss', 'booking-web', 'booking-api', 'notes', 'personal-site']);
    expect(rows.every((r) => r.getAttribute('aria-checked') === 'false')).toBe(true);
    expect(rows[3]!.getAttribute('aria-disabled')).toBe('true');
    expect(rows[3]!.textContent).toContain('Not a git repository');
    rows[3]!.click();
    rows[0]!.focus();
    await userEvent.keyboard(' {ArrowDown} ');
    expect(rows[0]!.getAttribute('aria-checked')).toBe('true');
    expect(rows[1]!.getAttribute('aria-checked')).toBe('true');
    expect(rows[3]!.getAttribute('aria-checked')).toBe('false');
    await userEvent.keyboard('{Meta>}{Enter}{/Meta}');
    await wait(30);
    expect(app.tasks.repos.map((r) => r.name)).toEqual(['airboss', 'booking-web']);
  });
});
