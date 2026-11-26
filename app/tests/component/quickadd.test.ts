import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import QuickAdd from '../../src/windows/quickadd/QuickAdd.svelte';
import { MockBackend } from '../../src/lib/mock/backend';
import { loadFixture } from '../../src/lib/mock/fixtures';
import { setTransport } from '../../src/lib/api/transport';
import { setClockOffset } from '../../src/lib/logic/clock';
import { fast } from './setup';

const settle = () => new Promise((r) => setTimeout(r, 50));

describe('quick add data (hands-on QA: stale repos)', () => {
  it('reloads repos and parents each time the panel gains focus', async () => {
    const b = new MockBackend(loadFixture('design'), { delays: fast });
    setClockOffset(b.s.nowMs - Date.now());
    setTransport(b);
    const invoke = vi.spyOn(b, 'invoke');
    const loads = () => invoke.mock.calls.filter(([c]) => c === 'quickadd_data').length;

    await render(QuickAdd, { native: false });
    await settle();
    const atMount = loads();
    expect(atMount).toBeGreaterThan(0);

    // A repo or task added in the main window since: showing the panel focuses it.
    window.dispatchEvent(new Event('focus'));
    await settle();
    expect(loads()).toBe(atMount + 1);
  });
});
