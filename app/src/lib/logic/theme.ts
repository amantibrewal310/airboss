/** Settings › Appearance. System follows macOS; Dark and Light override it. */
export type Appearance = 'system' | 'dark' | 'light';
export type Theme = 'dark' | 'light';

export function resolveTheme(appearance: Appearance, systemIsDark: boolean): Theme {
  if (appearance === 'dark' || appearance === 'light') return appearance;
  return systemIsDark ? 'dark' : 'light';
}

/** A `#rrggbb` accent becomes the focus colour and a 14% wash for selection. */
export function accentVars(accent: string | null): Record<string, string | null> {
  if (!accent || !/^#[0-9a-f]{6}$/i.test(accent)) {
    return { '--sys-accent': null, '--sys-accent-soft': null };
  }
  const n = parseInt(accent.slice(1), 16);
  const rgb = `${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}`;
  return { '--sys-accent': accent, '--sys-accent-soft': `rgba(${rgb}, 0.22)` };
}

/**
 * Applies the theme to <html> and keeps following the system while Appearance is System.
 * Returns a function that stops following.
 */
export function applyAppearance(
  root: HTMLElement,
  appearance: Appearance,
  onTheme?: (theme: Theme) => void,
): () => void {
  const media = window.matchMedia('(prefers-color-scheme: dark)');
  const set = () => {
    const theme = resolveTheme(appearance, media.matches);
    root.dataset.theme = theme;
    onTheme?.(theme);
  };
  set();
  if (appearance !== 'system') return () => {};
  media.addEventListener('change', set);
  return () => media.removeEventListener('change', set);
}

export function applyAccent(root: HTMLElement, accent: string | null): void {
  for (const [name, value] of Object.entries(accentVars(accent))) {
    if (value === null) root.style.removeProperty(name);
    else root.style.setProperty(name, value);
  }
}
