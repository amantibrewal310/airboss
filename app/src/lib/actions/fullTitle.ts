/**
 * A tooltip only when the text is cut off: on hover or focus, sets `title` to the full text if the
 * element is truncated (its content is wider than its box), and removes it otherwise. Use on any
 * element with `text-overflow: ellipsis`.
 */
export function isTruncated(el: HTMLElement): boolean {
  return el.scrollWidth > el.clientWidth + 0.5;
}

export function syncTitle(el: HTMLElement, text: string): void {
  if (text && isTruncated(el)) el.title = text;
  else el.removeAttribute('title');
}

export function fullTitle(el: HTMLElement, text: string) {
  let current = text;
  const sync = () => syncTitle(el, current);
  el.addEventListener('pointerenter', sync);
  el.addEventListener('focus', sync);
  return {
    update(next: string) {
      current = next;
      if (el.hasAttribute('title')) sync();
    },
    destroy() {
      el.removeEventListener('pointerenter', sync);
      el.removeEventListener('focus', sync);
    },
  };
}
