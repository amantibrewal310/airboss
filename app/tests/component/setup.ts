import { MockBackend, type MockDelays } from '../../src/lib/mock/backend';
import { loadFixture, type FixtureName } from '../../src/lib/mock/fixtures';
import { setTransport } from '../../src/lib/api/transport';
import { setClockOffset } from '../../src/lib/logic/clock';
import { App } from '../../src/lib/state/app.svelte';
import '../../src/styles/tokens.css';
import '../../src/styles/base.css';

export const fast: Partial<MockDelays> = { launch: 30, attention: 20, reject: 150, lookup: 10 };

/** A started controller on the mock backend. */
export async function startApp(fixture: FixtureName = 'design', delays: Partial<MockDelays> = fast) {
  const backend = new MockBackend(loadFixture(fixture), { delays });
  setClockOffset(backend.s.nowMs - Date.now());
  setTransport(backend);
  const app = new App();
  await app.start(false);
  return { app, backend };
}

export const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
