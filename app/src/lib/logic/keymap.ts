/*
  Keys as short strings: modifiers in a fixed order (ctrl, alt, shift, mod) then the key, e.g.
  "mod+enter", "shift+mod+enter", "alt+mod+up", "s", "?", and sequences "g n". Letters come
  from `code`, so ⌥N is "alt+n" on any layout (on a Mac, ⌥N types "˜").
*/

export interface KeyLike {
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const NAMED: Record<string, string> = {
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  Enter: 'enter',
  NumpadEnter: 'enter',
  Escape: 'esc',
  Backspace: 'backspace',
  Delete: 'backspace',
  Tab: 'tab',
  ' ': 'space',
  Spacebar: 'space',
  F6: 'f6',
  Home: 'home',
  End: 'end',
  PageUp: 'pageup',
  PageDown: 'pagedown',
};

/** The canonical string for a key press, or '' for a lone modifier. */
export function keyOf(e: KeyLike): string {
  if (['Meta', 'Control', 'Alt', 'Shift', 'CapsLock', 'Fn'].includes(e.key)) return '';
  let key: string;
  let shift = e.shiftKey;
  if (e.key === '?' || (e.code === 'Slash' && e.shiftKey)) {
    key = '?';
    shift = false;
  } else if (NAMED[e.key]) key = NAMED[e.key]!;
  else if (NAMED[e.code]) key = NAMED[e.code]!;
  else if (e.code.startsWith('Key')) key = e.code.slice(3).toLowerCase();
  else if (e.code.startsWith('Digit')) key = e.code.slice(5);
  else if (e.code === 'Comma') key = 'comma';
  else if (e.code === 'Period') key = 'period';
  else key = e.key.toLowerCase();
  const mods: string[] = [];
  if (e.ctrlKey) mods.push('ctrl');
  if (e.altKey) mods.push('alt');
  if (shift) mods.push('shift');
  if (e.metaKey) mods.push('mod');
  return [...mods, key].join('+');
}

const CAP: Record<string, string> = {
  mod: '⌘',
  ctrl: '⌃',
  alt: '⌥',
  shift: '⇧',
  enter: '↩',
  up: '↑',
  down: '↓',
  left: '←',
  right: '→',
  backspace: '⌫',
  esc: 'esc',
  space: 'Space',
  tab: '⇥',
  f6: 'F6',
  comma: ',',
  period: '.',
  home: 'Home',
  end: 'End',
};

/** Key caps for display: "mod+enter" → ["⌘", "↩"]; "g n" → ["G", "N"]. */
export function capsOf(keys: string): string[] {
  return keys
    .split(' ')
    .flatMap((chord) => chord.split('+'))
    .filter(Boolean)
    .map((k) => CAP[k] ?? k.toUpperCase());
}

/** One label for menus and tooltips: "⇧⌘↩", "G N". */
export function keyLabel(keys: string): string {
  return keys.includes(' ') ? capsOf(keys).join(' ') : capsOf(keys).join('');
}

/** Tauri accelerator text ("Control+Alt+N") as caps: "⌃⌥N". */
export function acceleratorLabel(acc: string): string {
  const map: Record<string, string> = {
    control: '⌃',
    ctrl: '⌃',
    alt: '⌥',
    option: '⌥',
    shift: '⇧',
    command: '⌘',
    cmd: '⌘',
    super: '⌘',
    commandorcontrol: '⌘',
    space: 'Space',
  };
  return acc
    .split('+')
    .map((p) => map[p.toLowerCase()] ?? p.replace(/^Key/, '').toUpperCase())
    .join('');
}

/** A key press as a Tauri accelerator ("Control+Alt+N"), for the hotkey recorder. */
export function acceleratorOf(e: KeyLike): string | null {
  const k = keyOf(e);
  if (!k) return null;
  const parts = k.split('+');
  const key = parts.pop()!;
  if (!parts.length) return null;
  const names: Record<string, string> = { ctrl: 'Control', alt: 'Alt', shift: 'Shift', mod: 'Command' };
  const keyName = key === 'space' ? 'Space' : key.length === 1 ? key.toUpperCase() : null;
  if (!keyName) return null;
  return [...parts.map((p) => names[p]!), keyName].join('+');
}

/** macOS uses these by default; Settings refuses them with the reason. */
export const RESERVED_HOTKEYS: Record<string, string> = {
  'Control+Space': 'macOS uses ⌃Space to switch input sources.',
  'Control+Alt+Space': 'macOS uses ⌃⌥Space to select the next input source.',
  'Command+Space': 'macOS uses ⌘Space for Spotlight.',
  'Alt+Command+Space': 'macOS uses ⌥⌘Space for a Finder search window.',
  'Control+Command+Space': 'macOS uses ⌃⌘Space for the Character Viewer.',
};

/**
 * G then N / L / R / A / P: a two-key sequence. `feed` returns the full key to dispatch,
 * '' while it waits for the second key, or the key itself.
 */
export class Sequencer {
  private pending: string | null = null;
  private at = 0;
  constructor(
    private readonly prefixes: string[] = ['g'],
    private readonly timeoutMs = 1000,
    private readonly clock: () => number = () => Date.now(),
  ) {}

  feed(key: string): string {
    const t = this.clock();
    if (this.pending && t - this.at <= this.timeoutMs) {
      const seq = `${this.pending} ${key}`;
      this.pending = null;
      return seq;
    }
    this.pending = null;
    if (this.prefixes.includes(key)) {
      this.pending = key;
      this.at = t;
      return '';
    }
    return key;
  }

  get waiting(): boolean {
    return this.pending !== null && this.clock() - this.at <= this.timeoutMs;
  }

  reset(): void {
    this.pending = null;
  }
}
