import './styles/tokens.css';
import './styles/base.css';
import { mount } from 'svelte';
import AppView from './windows/main/App.svelte';
import { App } from './lib/state/app.svelte';
import { setTransport } from './lib/api/transport';
import { setClockOffset } from './lib/logic/clock';

async function main() {
  if (__MOCK__) {
    const { createMockBackend, createRecordedTransport, fixtureFromUrl, isRecorded, RECORDED_NOW } = await import('./lib/mock');
    if (isRecorded(location.search)) {
      // Rust's recording, taken at DESIGN's "now".
      setClockOffset(Date.parse(RECORDED_NOW) - Date.now());
      setTransport(createRecordedTransport());
    } else {
      const backend = createMockBackend(fixtureFromUrl(location.search));
      setClockOffset(backend.s.nowMs - Date.now());
      setTransport(backend);
    }
  } else {
    const { tauriTransport } = await import('./lib/api/tauri');
    setTransport(tauriTransport);
  }
  const target = document.getElementById('app');
  if (!target) throw new Error('missing #app');
  const app = new App();
  mount(AppView, { target, props: { app } });
  // In the mock, ?view= and ?select= start somewhere other than Needs me (for screenshots).
  const params = new URLSearchParams(location.search);
  const select = params.get('select');
  await app.start(!__MOCK__, __MOCK__ ? { view: params.get('view'), select: select ? Number(select) : null } : undefined);
}

void main();
