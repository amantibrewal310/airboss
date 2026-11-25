import './styles/tokens.css';
import './styles/base.css';
import { mount } from 'svelte';
import QuickAdd from './windows/quickadd/QuickAdd.svelte';
import { setTransport } from './lib/api/transport';
import { applyAppearance } from './lib/logic/theme';

/** The quick-add panel's own small bundle. */
async function main() {
  if (__MOCK__) {
    const { createMockBackend, fixtureFromUrl } = await import('./lib/mock');
    setTransport(createMockBackend(fixtureFromUrl(location.search)));
  } else {
    const { tauriTransport } = await import('./lib/api/tauri');
    setTransport(tauriTransport);
  }
  let appearance: 'system' | 'dark' | 'light' = 'system';
  try {
    const { api } = await import('./lib/api');
    appearance = (await api.getSettings()).appearance;
  } catch {
    // System it is.
  }
  applyAppearance(document.documentElement, appearance);
  const target = document.getElementById('app');
  if (!target) throw new Error('missing #app');
  mount(QuickAdd, { target, props: { native: !__MOCK__ } });
}

void main();
