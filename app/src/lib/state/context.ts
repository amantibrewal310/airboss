import { getContext, setContext } from 'svelte';
import type { App } from './app.svelte';

const KEY = Symbol('app');

export function provideApp(app: App): void {
  setContext(KEY, app);
}

/** The main window's controller, for components inside it. */
export function useApp(): App {
  const app = getContext<App | undefined>(KEY);
  if (!app) throw new Error('useApp() outside the main window');
  return app;
}
