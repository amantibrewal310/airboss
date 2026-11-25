import { describe, expect, it } from 'vitest';
import { COMMAND_IDS } from '../../src/lib/logic/commands';
import menu from '../../src-tauri/src/menu.rs?raw';
import native from '../../src-tauri/src/native.rs?raw';
import appSource from '../../src/lib/state/app.svelte.ts?raw';

/**
 * Native menu items send their id as `menu:action` unless Rust handles them itself
 * (native.rs). Every id they can send must be a command the UI runs.
 */
describe('native menu ids', () => {
  const consts = new Map([...menu.matchAll(/pub const ([A-Z_]+): &str = "([a-z_:]+)";/g)].map((m) => [m[1]!, m[2]!]));
  const items = [...menu.matchAll(/item\(\s*"([a-z_]+)"/g)].map((m) => m[1]!);
  const withIds = [...menu.matchAll(/with_id\(\s*app,\s*ids::([A-Z_]+)/g)].map((m) => consts.get(m[1]!)!);
  // The ids native.rs answers itself, before the catch-all that forwards to the UI.
  const handled = new Set([...native.matchAll(/ids::([A-Z_]+)\s*(?:=>|\|)/g)].map((m) => consts.get(m[1]!)!));

  it('finds the menu', () => {
    expect(items.length).toBeGreaterThan(20);
    expect(withIds).toContain('undo');
    expect(handled).toContain('install_cli');
  });

  it('every id sent to the UI is a command it knows', () => {
    const sent = [...new Set([...items, ...withIds])].filter((id) => !handled.has(id));
    const known = new Set<string>(COMMAND_IDS);
    expect(sent.filter((id) => !known.has(id))).toEqual([]);
    // …and one `runCommand` acts on.
    expect(sent.filter((id) => !appSource.includes(`case '${id}':`))).toEqual([]);
  });
});
