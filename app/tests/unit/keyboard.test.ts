import { describe, expect, it } from 'vitest';
import { acceleratorLabel, acceleratorOf, capsOf, keyOf, keyLabel, Sequencer, type KeyLike } from '../../src/lib/logic/keymap';
import { allowed, type KeyGroup, type Zone } from '../../src/lib/logic/focus';
import { COMMANDS, CONTEXT_MENU, resolve, sheetRows } from '../../src/lib/logic/commands';

const k = (key: string, code: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key, code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods,
});

describe('keymap', () => {
  it('names keys with modifiers in a fixed order', () => {
    expect(keyOf(k('Enter', 'Enter', { metaKey: true }))).toBe('mod+enter');
    expect(keyOf(k('Enter', 'Enter', { metaKey: true, shiftKey: true }))).toBe('shift+mod+enter');
    expect(keyOf(k('ArrowUp', 'ArrowUp', { metaKey: true, altKey: true }))).toBe('alt+mod+up');
    expect(keyOf(k('s', 'KeyS'))).toBe('s');
    expect(keyOf(k(' ', 'Space'))).toBe('space');
    expect(keyOf(k('Backspace', 'Backspace', { metaKey: true }))).toBe('mod+backspace');
    expect(keyOf(k('C', 'KeyC', { metaKey: true, shiftKey: true }))).toBe('shift+mod+c');
    expect(keyOf(k('Meta', 'MetaLeft', { metaKey: true }))).toBe('');
  });
  it('reads letters from the key code, so ⌥N is alt+n on a Mac', () => {
    expect(keyOf(k('˜', 'KeyN', { altKey: true }))).toBe('alt+n');
  });
  it('reads ? without its shift', () => {
    expect(keyOf(k('?', 'Slash', { shiftKey: true }))).toBe('?');
  });
  it('shows keys as Mac key caps', () => {
    expect(capsOf('shift+mod+enter')).toEqual(['⇧', '⌘', '↩']);
    expect(capsOf('g n')).toEqual(['G', 'N']);
    expect(keyLabel('mod+backspace')).toBe('⌘⌫');
    expect(acceleratorLabel('Control+Alt+N')).toBe('⌃⌥N');
    expect(acceleratorOf(k('n', 'KeyN', { ctrlKey: true, altKey: true }))).toBe('Control+Alt+N');
    expect(acceleratorOf(k('n', 'KeyN'))).toBeNull();
  });
  it('runs G then N / L / R / A / P as one sequence, within a second', () => {
    let t = 0;
    const seq = new Sequencer(['g'], 1000, () => t);
    expect(seq.feed('g')).toBe('');
    expect(seq.feed('n')).toBe('g n');
    expect(seq.feed('g')).toBe('');
    t = 1500;
    expect(seq.feed('a')).toBe('a');
    expect(seq.feed('s')).toBe('s');
  });
});

/** SPEC-app Appendix C, second table: every row, every zone. */
describe('focus rules (Appendix C)', () => {
  const zones: Zone[] = ['list', 'panel', 'sidebar', 'field', 'overlay', 'quickadd'];
  const table: [KeyGroup, boolean[]][] = [
    ['global', [true, true, true, true, true, false]], // ⌘K, ⌘N (replace the overlay)
    ['window', [true, true, true, true, false, false]], // ⌘,
    ['task', [true, true, false, false, false, false]], // ⌘↩ ⇧⌘↩ ⌘D ⌘L ⌘B ⌘O ⇧⌘C ⌘⌫ ⌘↓ ⌘↑ ⌥⌘↑↓
    ['undo', [true, true, true, false, false, false]], // ⌘Z (a field keeps text undo)
    ['letter', [true, true, true, false, false, false]], // C S R G… ? Space
    ['nav', [true, false, false, false, false, false]], // arrows and ↩ drive the list
    ['esc', [true, true, true, false, false, false]], // fields and overlays handle their own
    ['f6', [true, true, true, true, false, false]],
  ];
  for (const [group, row] of table) {
    it(`${group}`, () => {
      expect(zones.map((z) => allowed(group, z))).toEqual(row);
    });
  }
});

describe('the command registry', () => {
  it('maps the Appendix C keys in list views', () => {
    const at = (key: string, zone: Zone = 'list', view: 'list' | 'needs' | 'plans' | 'page' = 'list', sel = true) =>
      resolve(key, zone, view, sel)?.id ?? null;
    expect(at('up')).toBe('up');
    expect(at('right')).toBe('open_fold');
    expect(at('enter')).toBe('rename');
    expect(at('space')).toBe('toggle_done');
    expect(at('s')).toBe('status');
    expect(at('r')).toBe('link_repos');
    expect(at('mod+enter')).toBe('do_it');
    expect(at('shift+mod+enter')).toBe('plan_it');
    expect(at('mod+down')).toBe('open_page');
    expect(at('alt+mod+up')).toBe('move_up');
    expect(at('mod+d')).toBe('add_waits_on');
    expect(at('mod+l')).toBe('add_link');
    expect(at('mod+b')).toBe('mark_blocked');
    expect(at('mod+o')).toBe('open_latest');
    expect(at('mod+backspace')).toBe('drop');
    expect(at('shift+mod+c')).toBe('copy_number');
    expect(at('tab')).toBe('to_panel');
    expect(at('g n')).toBe('go_needs');
    expect(at('c')).toBe('new_task');
    expect(at('alt+n')).toBe('new_task');
    expect(at('?')).toBe('shortcuts');
  });

  it('maps Needs me, Plans and the page', () => {
    expect(resolve('enter', 'list', 'needs', true)?.id).toBe('needs_main');
    expect(resolve('mod+enter', 'list', 'needs', true)?.id).toBe('needs_main');
    expect(resolve('mod+backspace', 'list', 'needs', true)?.id).toBe('needs_dismiss');
    expect(resolve('down', 'list', 'plans', false)?.id).toBe('plans_down');
    expect(resolve('enter', 'list', 'plans', false)?.id).toBe('plans_open');
    expect(resolve('mod+up', 'panel', 'page', true)?.id).toBe('back');
    expect(resolve('mod+enter', 'panel', 'page', true)?.id).toBe('do_it');
  });

  it('keeps single letters and task keys out of fields, and lets ⌘K and ⌘N through', () => {
    expect(resolve('s', 'field', 'list', true)).toBeNull();
    expect(resolve('mod+enter', 'field', 'list', true)).toBeNull();
    expect(resolve('mod+z', 'field', 'list', true)).toBeNull();
    expect(resolve('mod+k', 'field', 'list', true)?.id).toBe('jump');
    expect(resolve('mod+n', 'overlay', 'list', true)?.id).toBe('new_task');
    expect(resolve('mod+comma', 'overlay', 'list', true)).toBeNull();
    expect(resolve('up', 'sidebar', 'list', true)).toBeNull();
    expect(resolve('mod+enter', 'sidebar', 'list', true)).toBeNull();
  });

  it('needs a selection for task commands', () => {
    expect(resolve('space', 'list', 'list', false)).toBeNull();
    expect(resolve('up', 'list', 'list', false)?.id).toBe('up');
  });

  it('gives every bound key one command per view (no clashes)', () => {
    const seen = new Map<string, string>();
    for (const c of COMMANDS) {
      for (const [key] of c.bind) {
        for (const v of c.views ?? ['list', 'needs', 'plans', 'page']) {
          const slot = `${key}@${v}`;
          expect(seen.get(slot), `${slot}: ${c.id} vs ${seen.get(slot)}`).toBeUndefined();
          seen.set(slot, c.id);
        }
      }
    }
  });

  it('builds the sheet and the context menu from the registry', () => {
    expect(sheetRows('ctrl+alt+n').map((r) => r.label)).toContain('Quick add from any app');
    expect(CONTEXT_MENU.filter((x) => x !== '-')).toHaveLength(12);
  });
});
