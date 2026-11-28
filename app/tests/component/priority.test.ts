import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { userEvent } from 'vitest/browser';
import Harness from './Harness.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import TaskPanel from '../../src/windows/main/TaskPanel.svelte';
import { startApp, wait } from './setup';
import { CONTEXT_MENU, resolve } from '../../src/lib/logic/commands';

/** Task 70: set a task's priority from the panel's Props, P, the Task menu and the right-click menu. */
async function open(id = 146) {
  const { app, backend } = await startApp('design');
  app.go('all');
  app.select(id, { open: true });
  await render(Harness, { app, component: TaskPanel });
  await render(Harness, { app, component: Overlays });
  await wait(30);
  const line = () => [...document.querySelectorAll<HTMLElement>('.props .p')].find((p) => p.querySelector('.l')?.textContent === 'Priority')!;
  return { app, backend, line };
}

const items = () => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((e) => `${e.querySelector('.t')!.textContent} ${e.querySelector('.k')!.textContent}`);
const priorityCalls = (backend: { calls: { command: string; args: unknown }[] }) =>
  backend.calls.filter((c) => c.command === 'set_priority').map((c) => c.args);

describe('Priority', () => {
  it('shows None in Props, and clicking it offers P0 to P3 and None', async () => {
    const { line } = await open();
    expect(line().querySelector('.v')!.textContent!.trim()).toBe('None · set a priority');
    line().querySelector<HTMLButtonElement>('button')!.click();
    await wait(30);
    expect(items()).toEqual(['P0 · most urgent 0', 'P1 1', 'P2 2', 'P3 · least urgent 3', 'None ⌫']);
  });

  it('a number key sets it, the tag shows it, and the menu reopens on it', async () => {
    const { app, backend, line } = await open();
    app.runCommand('priority');
    await wait(30);
    await userEvent.keyboard('1');
    await wait(50);
    expect(priorityCalls(backend)).toEqual([{ id: 146, priority: 'p1' }]);
    expect(app.tasks.rows.get(146)!.priority).toBe('p1');
    const tag = line().querySelector<HTMLElement>('.tag.pri')!;
    expect(tag.textContent).toBe('P1');
    expect(tag.classList.contains('p1')).toBe(true);
    tag.click();
    await wait(30);
    expect(document.querySelector('[role="menuitem"].hl')!.textContent).toContain('P1');
  });

  it('⌫ clears it, and picking the one it has writes nothing new', async () => {
    const { app, backend } = await open();
    await app.setPriority(146, 'p2');
    await app.setPriority(146, 'p2');
    expect(app.tasks.rows.get(146)!.priority).toBe('p2');
    app.runCommand('priority');
    await wait(30);
    await userEvent.keyboard('{Backspace}');
    await wait(50);
    expect(app.tasks.rows.get(146)!.priority).toBeNull();
    expect(priorityCalls(backend).at(-1)).toEqual({ id: 146, priority: null });
    const edits = backend.s.events.filter((e) => e.task === 146 && e.kind === 'edited').map((e) => e.body);
    expect(edits).toEqual(['priority: P2', 'priority cleared']);
  });

  it('P opens it from the list, and the right-click menu offers it', () => {
    expect(resolve('p', 'list', 'list', true)?.id).toBe('priority');
    expect(resolve('p', 'list', 'list', false)).toBeNull();
    expect(CONTEXT_MENU.indexOf('priority')).toBe(CONTEXT_MENU.indexOf('status') + 1);
  });
});
