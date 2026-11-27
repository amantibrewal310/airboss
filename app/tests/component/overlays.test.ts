import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Harness from './Harness.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import StartInMenu from '../../src/windows/main/overlays/StartInMenu.svelte';
import NewTask from '../../src/windows/shared/NewTask.svelte';
import { startApp, wait } from './setup';
import { loadFixture } from '../../src/lib/mock/fixtures';

describe('StatusMenu', () => {
  it('opens on the task’s current status, not Todo (hands-on QA #21)', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    expect(app.tasks.rows.get(142)!.status).toBe('doing');
    app.open({ kind: 'status', task: 142, at: { x: 300, y: 200 } });
    await wait(30);
    const hl = document.querySelector<HTMLElement>('[role="menuitem"].hl')!;
    expect(hl.textContent).toContain('In progress');
    expect(document.activeElement).toBe(hl);
    // ↓ moves on from there.
    await userEvent.keyboard('{ArrowDown}');
    expect(document.querySelector('[role="menuitem"].hl')!.textContent).toContain('In review');
  });

  it('sets a status with 1–6', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'status', task: 146, at: { x: 300, y: 200 } });
    await wait(30);
    expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(6);
    await userEvent.keyboard('2');
    await wait(30);
    expect(app.tasks.rows.get(146)!.status).toBe('doing');
    expect(app.toasts.list.at(-1)!.text).toBe('Moved #146 to In progress');
  });

  it('asks why for Blocked and won’t save an empty reason', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'status', task: 146, at: { x: 300, y: 200 } });
    await wait(30);
    await userEvent.keyboard('4');
    await wait(30);
    const input = document.querySelector<HTMLInputElement>('#blocked-reason')!;
    expect(document.activeElement).toBe(input);
    await userEvent.keyboard('{Enter}');
    await wait(20);
    expect(app.tasks.rows.get(146)!.status).toBe('todo');
    await userEvent.keyboard('Waiting on the mail team{Enter}');
    await wait(30);
    expect(app.tasks.rows.get(146)!.status).toBe('blocked');
    expect(app.tasks.rows.get(146)!.blocked_reason).toBe('Waiting on the mail team');
  });

  it('confirms Dropped, naming the open subtasks; Esc cancels', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'status', task: 142, at: { x: 300, y: 200 } });
    await wait(30);
    await userEvent.keyboard('6');
    await wait(30);
    expect(document.querySelector('[role="dialog"] h2')!.textContent).toBe('Drop 142 and its 4 open subtasks?');
    await userEvent.keyboard('{Escape}');
    await wait(30);
    expect(app.tasks.rows.get(142)!.status).toBe('doing');
    app.open({ kind: 'drop', task: 142, open: 4 });
    await wait(30);
    await userEvent.keyboard('{Enter}');
    await wait(30);
    // Its open steps are dropped and one was done, so it shows done (SPEC-core rule 10).
    expect(app.tasks.rows.get(142)!.status).toBe('done');
  });

  it('offers Accept and Reject on a proposed task', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'status', task: 153, at: { x: 300, y: 200 } });
    await wait(30);
    const items = [...document.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent?.replace(/\s+/g, ' ').trim());
    expect(items).toEqual(['Accept 1', 'Reject 2']);
  });
});

describe('StartInMenu', () => {
  it('starts in a repo or as research; ↓ and ↩ pick, Esc closes', async () => {
    const onpick = vi.fn();
    const onclose = vi.fn();
    const choices = loadFixture('design').repos;
    const { app } = await startApp();
    await render(Harness, { props: { app, component: StartInMenu, props: { task: 206, x: 200, y: 200, choices, onpick, onclose } } });
    await wait(20);
    const labels = [...document.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent?.trim());
    expect(labels).toEqual(['Start in airboss', 'Start in booking-web', 'Start in booking-api', 'Start in dotfiles', 'Add another repo…', 'No repo: research only']);
    // ↩ waits out the 300 ms after opening, so the key that opened the menu can't pick from it.
    await userEvent.keyboard('{ArrowDown}');
    await wait(320);
    await userEvent.keyboard('{Enter}');
    expect(onpick).toHaveBeenCalledWith({ repo: 1 });
  });
});

describe('Jump', () => {
  it('jumps to a task by id and reveals it', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'jump', mode: 'jump' });
    await wait(30);
    await userEvent.keyboard('208');
    await wait(20);
    expect(document.querySelector('[role="option"][aria-selected="true"]')!.textContent).toContain('Backfill old refunds');
    await userEvent.keyboard('{Enter}');
    await wait(30);
    expect(app.ui.view).toBe('all');
    expect(app.ui.selected).toBe(208);
    expect(app.ui.folded.has(200)).toBe(false);
  });

  it('offers to create the text as a task when nothing matches', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'jump', mode: 'jump' });
    await wait(30);
    await userEvent.keyboard('zzqx');
    await wait(20);
    expect(document.querySelector('[role="option"]')!.textContent).toContain('Create task “zzqx”');
  });

  it('greys out invalid targets in picker mode, with the reason', async () => {
    const { app } = await startApp();
    await render(Harness, { app, component: Overlays });
    app.open({ kind: 'jump', mode: 'waits_on', task: 144 });
    await wait(30);
    await userEvent.keyboard('142');
    await wait(20);
    const opt = document.querySelector('[role="option"]')!;
    expect(opt.getAttribute('aria-disabled')).toBe('true');
    expect(opt.textContent).toContain('Its own parent');
    await userEvent.keyboard('{Enter}');
    expect(app.ui.overlay).not.toBeNull();
  });
});

describe('NewTask', () => {
  const data = loadFixture('design');
  const parents = data.tasks.filter((t) => t.status === 'doing').map((t) => ({ id: t.id, title: t.title, status: t.status, waiting: false, depth: 0 }));

  async function setup() {
    const onsubmit = vi.fn();
    const oncancel = vi.fn();
    await render(NewTask, { repos: data.repos, parents, hotkey: 'Control+Alt+N', onsubmit, oncancel, inApp: false });
    const title = document.querySelector<HTMLInputElement>('input[aria-label="Title"]')!;
    title.focus();
    return { onsubmit, oncancel, title };
  }

  it('moves from the title to the notes with ↓, and back with ↑ on the first line', async () => {
    const { title } = await setup();
    await userEvent.keyboard('Fix double booking{ArrowDown}');
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Notes');
    await userEvent.keyboard('first line{ArrowUp}');
    expect(document.activeElement).toBe(title);
  });

  it('opens repos with Tab and parent with Shift-Tab', async () => {
    await setup();
    await userEvent.keyboard('{Tab}');
    expect(document.querySelector('[role="listbox"]')!.getAttribute('aria-label')).toBe('Link repos…');
    await userEvent.keyboard('{Enter}');
    expect(document.querySelector('.pp.set')).not.toBeNull();
    await userEvent.keyboard('{Shift>}{Tab}{/Shift}');
    expect(document.querySelector('[role="listbox"]')!.getAttribute('aria-label')).toBe('Parent…');
    await userEvent.keyboard('{Escape}');
    expect(document.querySelector('[role="listbox"]')).toBeNull();
  });

  it('attaches a pasted Slack link and a t3code link as pills, keeping the rest', async () => {
    const { title } = await setup();
    const paste = (text: string) => {
      const dt = new DataTransfer();
      dt.setData('text/plain', text);
      title.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    };
    paste('Double booking https://acme.slack.com/archives/C04/p123');
    paste('t3code://threads/env-local/slot-picker');
    await new Promise((r) => setTimeout(r, 10));
    expect(title.value).toBe('Double booking');
    const pills = [...document.querySelectorAll('.pp.link')].map((p) => p.textContent?.trim());
    expect(pills).toEqual(['Slack thread', 'T3 · slot-picker']);
  });

  it('creates with ↩ and creates-and-starts with ⌘↩; an empty title can’t be created', async () => {
    const { onsubmit } = await setup();
    await userEvent.keyboard('{Enter}');
    expect(onsubmit).not.toHaveBeenCalled();
    await userEvent.keyboard('Fix double booking{Meta>}{Enter}{/Meta}');
    expect(onsubmit).toHaveBeenCalledWith(expect.objectContaining({ title: 'Fix double booking', then: 'do_it' }));
  });
});
