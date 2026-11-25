import { describe, expect, it } from 'vitest';
import { accentVars, resolveTheme } from '../../src/lib/logic/theme';

describe('resolveTheme', () => {
  it('follows the system by default', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
  });
  it('lets Dark and Light override the system', () => {
    expect(resolveTheme('dark', false)).toBe('dark');
    expect(resolveTheme('light', true)).toBe('light');
  });
});

describe('accentVars', () => {
  it('turns a system accent into focus and selection colours', () => {
    expect(accentVars('#0a84ff')).toEqual({
      '--sys-accent': '#0a84ff',
      '--sys-accent-soft': 'rgba(10, 132, 255, 0.22)',
    });
  });
  it('keeps indigo when there is no accent (Multicolor) or it is malformed', () => {
    expect(accentVars(null)).toEqual({ '--sys-accent': null, '--sys-accent-soft': null });
    expect(accentVars('blue')).toEqual({ '--sys-accent': null, '--sys-accent-soft': null });
  });
});
