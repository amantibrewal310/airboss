import { describe, expect, it } from 'vitest';
import { launcherMissingText, mergeConnections } from '../../src/lib/logic/connections';
import type { Connection } from '../../src/lib/types';

/** SPEC-herdr "app": how Herdr terminals and the launcher read. */

describe('connections with Herdr', () => {
  const c = (name: string, ok: boolean, state: string): Connection => ({ name, ok, state, detail: `${name} detail`, fix: ok ? null : 'fix it' });

  it('puts Herdr after T3 Code', () => {
    const merged = mergeConnections([c('GitHub', true, 'Signed in'), c('Herdr', true, 'Ready'), c('T3 Code', true, 'Found')], 'unknown');
    expect(merged.map((m) => m.name)).toEqual(['T3 Code', 'Herdr', 'GitHub']);
  });

  it("doesn't put back a T3 Code row Rust left out for someone on Herdr", () => {
    const missing = { missing: { reason: 'T3 Code isn’t installed.' } };
    expect(mergeConnections([c('Herdr', true, 'Ready')], missing, { launcher: 'herdr' }).map((m) => m.name)).toEqual(['Herdr']);
    // On T3 Code, its missing row is still added.
    expect(mergeConnections([c('Herdr', true, 'Ready')], missing, { launcher: 't3' }).map((m) => m.name)).toEqual(['T3 Code', 'Herdr']);
  });

  it('names the launcher when it is missing', () => {
    expect(launcherMissingText('herdr')).toBe('Do it and Plan it need Herdr. Install it, then choose Check again.');
    expect(launcherMissingText('t3')).toBe('Do it and Plan it need T3 Code. Install it, then choose Check again.');
  });
});
