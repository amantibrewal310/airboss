/*
  Focus helpers. With macOS Keyboard navigation off, WebKit's Tab skips buttons, so the app moves
  focus itself: Tab always goes to the next focusable element in document order (buttons
  included), wrapping inside a trap (menus, popovers, modals) and inside the window otherwise.
*/

const FOCUSABLE = 'button:not([disabled]), a[href], input:not([disabled]):not([type="hidden"]), textarea:not([disabled]), select:not([disabled]), [tabindex]';

function visible(el: HTMLElement): boolean {
  if (!el.getClientRects().length) return false;
  const s = getComputedStyle(el);
  return s.visibility !== 'hidden' && s.display !== 'none';
}

/** The elements Tab visits inside `root`, in document order. */
export function focusables(root: ParentNode): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => el.tabIndex >= 0 && !el.closest('[inert], [aria-hidden="true"]') && visible(el),
  );
}

/** The next (or previous) element after `from` in document order, wrapping round. */
export function nextFocusable(list: HTMLElement[], from: Element | null, back: boolean): HTMLElement | undefined {
  if (!list.length) return undefined;
  const at = from ? list.indexOf(from as HTMLElement) : -1;
  if (at !== -1) return list[(at + (back ? -1 : 1) + list.length) % list.length];
  if (!from || from === document.body) return back ? list[list.length - 1] : list[0];
  // `from` isn't a Tab stop itself (a container with tabindex=-1): go by document position.
  const after = (el: HTMLElement) => !!(from.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING);
  if (back) return [...list].reverse().find((el) => !after(el) && !el.contains(from)) ?? list[list.length - 1];
  return list.find((el) => after(el) && !from.contains(el)) ?? list.find((el) => after(el)) ?? list[0];
}

/** Handles Tab and Shift-Tab inside `root`: always the app's own order, never the browser's. */
export function trapTab(e: KeyboardEvent, root: HTMLElement): void {
  if (e.key !== 'Tab' || e.metaKey || e.ctrlKey || e.altKey) return;
  e.preventDefault();
  const target = nextFocusable(focusables(root), document.activeElement, e.shiftKey);
  (target ?? root).focus();
}

/** Calls `onOutside` for a pointer press outside `root`. */
export function outsideClick(root: () => HTMLElement | undefined, onOutside: () => void): () => void {
  const handler = (e: PointerEvent) => {
    const el = root();
    if (el && !el.contains(e.target as Node)) onOutside();
  };
  window.addEventListener('pointerdown', handler, true);
  return () => window.removeEventListener('pointerdown', handler, true);
}
