import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Harness from './Harness.svelte';
import Connections from '../../src/windows/main/Connections.svelte';
import { startApp, wait } from './setup';

/** The connections list explains itself (owner's question, 26 Nov 2026: "What is this"). */
describe('the connections list', () => {
  it('has a heading that says what it is, and plain states', async () => {
    const { app } = await startApp('design');
    await render(Harness, { app, component: Connections });
    app.open({ kind: 'connections' });
    await wait(30);
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    const title = document.getElementById(dialog.getAttribute('aria-labelledby')!)!;
    const what = document.getElementById(dialog.getAttribute('aria-describedby')!)!;
    expect(title.textContent).toBe('Connections');
    expect(what.textContent).toBe('The tools Airboss works with. Green ones work; orange ones need setting up.');
    // The heading comes before the rows.
    expect(dialog.firstElementChild!.contains(title)).toBe(true);
    const rows = [...dialog.querySelectorAll('.ci b')].map((b) => b.textContent);
    expect(rows).toContain('T3 Code · Connected');
    expect(rows.join(' ')).not.toContain('Live state');
  });
});
