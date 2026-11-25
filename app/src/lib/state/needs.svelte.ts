import type { NeedsChanged, NeedsItem, NeedsList } from '../types';

/** How long an acted-on item stays hidden if `attention` still lists it (SPEC-app). */
export const LOCAL_HIDE_MS = 2000;
export const EXIT_MS = 180;

export class NeedsStore {
  items = $state.raw<NeedsItem[]>([]);
  checkedAt = $state<string | null>(null);
  version = 0;
  /** key → time it may come back. */
  hidden = $state.raw(new Map<string, number>());
  /** Keys sliding out (the 180 ms exit). */
  leaving = $state.raw(new Set<string>());
  selectedKey = $state<string | null>(null);

  visible = $derived(this.items.filter((n) => !this.hidden.has(n.key)));
  count = $derived(this.visible.length);

  load(list: NeedsList): void {
    this.items = list.items;
    this.checkedAt = list.checked_at;
    this.version = list.version;
  }

  apply(c: NeedsChanged): boolean {
    if (c.version < this.version) return false;
    this.items = c.items;
    this.checkedAt = c.checked_at;
    this.version = c.version;
    // An item attention dropped no longer needs hiding.
    const keys = new Set(c.items.map((n) => n.key));
    if ([...this.hidden.keys()].some((k) => !keys.has(k))) {
      this.hidden = new Map([...this.hidden].filter(([k]) => keys.has(k)));
    }
    return true;
  }

  /** Slides an item out at once; it stays hidden until attention drops it, or `ms` at most. */
  hideLocally(key: string, ms = LOCAL_HIDE_MS, onExit?: () => void): void {
    this.leaving = new Set(this.leaving).add(key);
    setTimeout(() => {
      const leaving = new Set(this.leaving);
      leaving.delete(key);
      this.leaving = leaving;
      this.hidden = new Map(this.hidden).set(key, Date.now() + ms);
      onExit?.();
      setTimeout(() => this.unhide(key, true), ms);
    }, EXIT_MS);
  }

  /** Brings an item back: on undo, or when its local hiding expires and attention still lists it. */
  unhide(key: string, onlyIfExpired = false): void {
    const until = this.hidden.get(key);
    if (until === undefined) return;
    if (onlyIfExpired && Date.now() < until - 5) return;
    const next = new Map(this.hidden);
    next.delete(key);
    this.hidden = next;
  }

  forTask(id: number): NeedsItem[] {
    return this.visible.filter((n) => n.task === id);
  }
}
