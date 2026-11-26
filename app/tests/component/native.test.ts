import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { tick } from 'svelte';
import Harness from './Harness.svelte';
import Sidebar from '../../src/windows/main/Sidebar.svelte';
import { MockBackend } from '../../src/lib/mock/backend';
import { loadFixture } from '../../src/lib/mock/fixtures';
import { setTransport } from '../../src/lib/api/transport';
import { setClockOffset } from '../../src/lib/logic/clock';
import { App } from '../../src/lib/state/app.svelte';
import { fast } from './setup';

function backend() {
  const b = new MockBackend(loadFixture('design'), { delays: fast });
  setClockOffset(b.s.nowMs - Date.now());
  setTransport(b);
}

const fakeLights = () => document.querySelectorAll('.lights i').length;

describe('traffic lights (hands-on QA #1)', () => {
  it('a native app draws no fake traffic lights, even when mounted before start()', async () => {
    backend();
    // main.ts mounts the window first and starts the controller after.
    const app = new App();
    await render(Harness, { app, component: Sidebar });
    await app.start(true);
    await tick();
    expect(fakeLights()).toBe(0);
  });

  it('knows it is native from construction, so the first frame has none either', async () => {
    backend();
    const app = new App({ native: true });
    await render(Harness, { app, component: Sidebar });
    expect(fakeLights()).toBe(0);
  });

  it('the browser mock still draws them', async () => {
    backend();
    const app = new App();
    await render(Harness, { app, component: Sidebar });
    await app.start(false);
    await tick();
    expect(fakeLights()).toBe(3);
  });
});
