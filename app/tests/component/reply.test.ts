import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import TaskPanel from '../../src/windows/main/TaskPanel.svelte';
import { startApp, wait } from './setup';

/** The Threads card's "what's left for you" part (fixture `full`: 128 MCP server, its agent settled with a reply). */
async function open(prepare?: (backend: Awaited<ReturnType<typeof startApp>>['backend']) => void) {
  const { app, backend } = await startApp('full');
  prepare?.(backend);
  app.go('all');
  app.select(128, { open: true });
  await render(Harness, { app, component: TaskPanel });
  await wait(60);
  const threads = document.querySelector<HTMLElement>('[aria-label="Threads"]')!;
  return { app, backend, threads };
}

const next = (root: HTMLElement) => root.querySelector<HTMLElement>('[aria-label="What the agent left for you"]');

describe('Next steps from the last reply', () => {
  it('shows the lead and the steps after it, not the rest of the reply', async () => {
    const { threads } = await open();
    const card = next(threads)!;
    expect(card.querySelector('.label span')!.textContent).toBe('Next on your side');
    const items = [...card.querySelectorAll('ol li')].map((li) => li.textContent!.trim());
    expect(items).toEqual([
      'Review and merge PR #31.',
      'Run airboss mcp install once, so T3 picks the server up.',
      'Decide whether propose_tasks should need your approval in the app.',
    ]);
    expect(card.textContent).toContain('Once you decide on approval, tell me and I will add it.');
    expect(card.textContent).not.toContain('The MCP server now answers');
  });

  it('Full reply shows all of it, and Show less goes back', async () => {
    const { threads } = await open();
    const card = next(threads)!;
    const toggle = card.querySelector<HTMLButtonElement>('.more')!;
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    toggle.click();
    await wait(20);
    expect(toggle.textContent!.trim()).toBe('Show less');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(card.querySelector('.label span')!.textContent).toBe('Last reply');
    expect(card.textContent).toContain('The MCP server now answers');
    toggle.click();
    await wait(20);
    expect(card.textContent).not.toContain('The MCP server now answers');
  });

  it('opens an https link in the reply through open_url', async () => {
    const { threads, backend } = await open();
    next(threads)!.querySelector<HTMLButtonElement>('.more')!.click();
    await wait(20);
    const link = [...threads.querySelectorAll<HTMLButtonElement>('button.a')].find((b) => b.textContent === 'PR #31')!;
    link.click();
    await wait(20);
    expect(backend.opened.at(-1)).toBe('https://github.com/airboss/airboss/pull/31');
  });

  it('is hidden while the agent works, since the reply is about the turn before', async () => {
    const { threads } = await open((backend) => {
      const t = [...backend.s.threads.values()].find((x) => x.view.key.ref === 'mcp-server')!;
      t.view = { ...t.view, state: 'running' };
    });
    expect(threads.textContent).toContain('mcp-server');
    expect(next(threads)).toBeNull();
  });
});
