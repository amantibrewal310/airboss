/*
  Focus zones and which key groups act in each (SPEC-app Appendix C, second table).
*/

import type { FocusZone } from '../types';

export type Zone = FocusZone;

/**
 * - `global`: ⌘K, ⌘N (they replace an open overlay)
 * - `window`: ⌘, (Settings)
 * - `task`: ⌘↩, ⇧⌘↩, ⌘D, ⌘L, ⌘B, ⌘O, ⇧⌘C, ⌘⌫, ⌘↓, ⌘↑, ⌥⌘↑↓
 * - `undo`: ⌘Z as the app's undo (a field keeps its own text undo)
 * - `letter`: single letters (C, S, R, G…, ?) and Space
 * - `nav`: ↑ ↓ → ← ↩ and Tab when they drive the list (in the sidebar and panel, controls own them)
 * - `esc`: Esc outside fields and overlays (they handle their own)
 * - `f6`: F6 to the newest toast and back
 */
export type KeyGroup = 'global' | 'window' | 'task' | 'undo' | 'letter' | 'nav' | 'esc' | 'f6';

const TABLE: Record<KeyGroup, Record<Zone, boolean>> = {
  global: { list: true, panel: true, sidebar: true, field: true, overlay: true, quickadd: false },
  window: { list: true, panel: true, sidebar: true, field: true, overlay: false, quickadd: false },
  task: { list: true, panel: true, sidebar: false, field: false, overlay: false, quickadd: false },
  undo: { list: true, panel: true, sidebar: true, field: false, overlay: false, quickadd: false },
  letter: { list: true, panel: true, sidebar: true, field: false, overlay: false, quickadd: false },
  nav: { list: true, panel: false, sidebar: false, field: false, overlay: false, quickadd: false },
  esc: { list: true, panel: true, sidebar: true, field: false, overlay: false, quickadd: false },
  f6: { list: true, panel: true, sidebar: true, field: true, overlay: false, quickadd: false },
};

export function allowed(group: KeyGroup, zone: Zone): boolean {
  return TABLE[group][zone];
}

const FIELD = 'input, textarea, select, [contenteditable=""], [contenteditable="true"]';

/** The zone of the focused element. An open overlay takes every key first. */
export function zoneOf(active: Element | null, overlayOpen: boolean): Zone {
  if (overlayOpen) return 'overlay';
  if (!active || active === document.body) return 'list';
  if (active.matches(FIELD) && !(active as HTMLInputElement).readOnly) return 'field';
  if (active.closest('[data-zone="overlay"]')) return 'overlay';
  if (active.closest('[data-zone="sidebar"]')) return 'sidebar';
  if (active.closest('[data-zone="panel"]')) return 'panel';
  return 'list';
}

/** In the panel, ↩ and Space press the focused button instead of acting on the task. */
export function controlOwnsKey(active: Element | null, key: string): boolean {
  if (key !== 'enter' && key !== 'space') return false;
  return !!active?.closest('button, a[href], [role="button"], [role="radio"], [role="checkbox"], [role="menuitem"], [role="option"], summary');
}
