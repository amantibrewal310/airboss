import type { Shape } from '../../../lib/logic/views';

/** Status icons as SVG text, for menu items built from raw snippets. Mirrors StatusIcon.svelte. */
export function shapeSvg(shape: Shape, size = 16): string {
  const ring = (c: string, dash = '') =>
    `<circle cx="8" cy="8" r="6.25" fill="none" stroke="${c}" stroke-width="1.6"${dash ? ` stroke-dasharray="${dash}"` : ''}/>`;
  const inner: Record<Shape, string> = {
    todo: ring('var(--st-todo)'),
    waiting: ring('var(--st-todo)', '2.4 2.2'),
    proposed: ring('var(--st-todo)', '1.2 2.2'),
    doing: ring('var(--st-doing)') + '<path d="M8 8V4.2a3.8 3.8 0 0 1 0 7.6Z" fill="var(--st-doing)"/>',
    in_review: ring('var(--st-review)') + '<path d="M8 8V4.2a3.8 3.8 0 1 1-3.8 3.8Z" fill="var(--st-review)"/>',
    blocked: ring('var(--st-blocked)') + '<path d="M5.5 8h5" stroke="var(--st-blocked)" stroke-width="2" stroke-linecap="round"/>',
    done: '<circle cx="8" cy="8" r="7" fill="var(--st-done)"/><path d="M5 8.2l2 2 4-4.3" fill="none" stroke="#fff" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>',
    dropped:
      ring('var(--st-todo)') +
      '<path d="M5.6 5.6l4.8 4.8M10.4 5.6l-4.8 4.8" stroke="var(--st-todo)" stroke-width="1.4" stroke-linecap="round"/>',
  };
  return `<svg width="${size}" height="${size}" viewBox="0 0 16 16" aria-hidden="true">${inner[shape]}</svg>`;
}

/** A line icon as SVG text (stroked in currentColor). */
export function iconSvg(path: string, size = 14): string {
  return `<svg width="${size}" height="${size}" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
}

/** A repo dot as HTML text. */
export function dotHtml(id: number): string {
  const n = (((id - 1) % 4) + 4) % 4 + 1;
  return `<span style="width:8px;height:8px;border-radius:50%;background:var(--repo-${n});display:inline-block"></span>`;
}
