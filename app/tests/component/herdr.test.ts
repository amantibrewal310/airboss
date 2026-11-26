import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import Overlays from '../../src/windows/main/Overlays.svelte';
import TaskPanel from '../../src/windows/main/TaskPanel.svelte';
import { startApp, wait } from './setup';

/** SPEC-herdr "app": Herdr as the second place agents start, from the window's side. */

const radio = (group: string, label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>(`[role="radiogroup"][aria-label="${group}"] [role="radio"]`)].find((b) => b.textContent === label)!;

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
});
