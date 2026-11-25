/*
  The browser's stand-in for Rust (`vite --mode mock`). `?fixture=` picks the data:
  design | full (default) | perf | empty | welcome | t3missing | nothing | dberror.
*/

import { MockBackend } from './backend';
import { isFixtureName, loadFixture, type FixtureName } from './fixtures';

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
