/*
  The browser's stand-in for Rust (`vite --mode mock`). `?fixture=` picks the data:
  design | full (default) | perf | empty | welcome | t3missing | herdr | nothing | dberror, or
  `recorded`: Rust's own answers (fixtures/recorded.json) through the strict replay transport.
*/

import type { Transport } from '../api/transport';
import { MockBackend } from './backend';
import { isFixtureName, loadFixture, loadRecording, type FixtureName } from './fixtures';
import { createReplayTransport } from './replay';

export { MockBackend, UnscriptedCall } from './backend';
export { createReplayTransport } from './replay';

export function fixtureFromUrl(search: string): FixtureName {
  const name = new URLSearchParams(search).get('fixture') ?? 'full';
  return isFixtureName(name) ? name : 'full';
}

export function createMockBackend(name: FixtureName): MockBackend {
  const backend = new MockBackend(loadFixture(name));
  (globalThis as { __airbossMock?: MockBackend }).__airbossMock = backend;
  return backend;
}

/** Calls the window makes on its own (focus, folds, sizing); the recording doesn't script them. */
export const CHROME_COMMANDS = new Set<string>(['set_focus_context', 'ui_ready', 'set_folded', 'quickadd_shown', 'quickadd_resize']);

/**
 * The UI on Rust's recorded data. Reads come from the recording; writes must follow its script
 * in order, exactly (anything else is reported to `onUnscripted` and fails like a Rust error).
 */
export function createRecordedTransport(onUnscripted: (what: string) => void = (w) => console.error(w)): Transport & { remaining(): number } {
  const replay = createReplayTransport(loadRecording());
  return {
    native: false,
    remaining: replay.remaining,
    invoke: (async (command: string, args: unknown) => {
      if (CHROME_COMMANDS.has(command)) return null;
      try {
        return await replay.invoke(command as never, args as never);
      } catch (e) {
        if ((e as Error)?.name !== 'UnscriptedCall') throw e;
        onUnscripted(String(e));
        throw { code: 'internal', message: 'That isn’t in the recorded script.', detail: String(e) };
      }
    }) as Transport['invoke'],
    listen: replay.listen,
  };
}

export function isRecorded(search: string): boolean {
  return new URLSearchParams(search).get('fixture') === 'recorded';
}

/** When the recording was taken: the needs inbox's `checked_at`. */
export const RECORDED_NOW = (loadRecording().reads.bootstrap as { needs: { checked_at: string } }).needs.checked_at;
