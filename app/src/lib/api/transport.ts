import type { ArgsOf, CommandName, EventName, Events, ResultOf } from './contract';

/** How the UI talks to Rust: Tauri's `invoke` and `listen`, or the mock in the browser. */
export interface Transport {
  invoke<K extends CommandName>(command: K, args: ArgsOf<K>): Promise<ResultOf<K>>;
  listen<E extends EventName>(event: E, handler: (payload: Events[E]) => void): Promise<() => void>;
  /** True inside the Tauri app (native traffic lights, drag regions, menus). */
  readonly native: boolean;
}

let current: Transport | null = null;

/** Set once at start-up by the entry point (Tauri or mock). */
export function setTransport(t: Transport): void {
  current = t;
}

export function transport(): Transport {
  if (!current) throw new Error('transport not set: call setTransport() at start-up');
  return current;
}
