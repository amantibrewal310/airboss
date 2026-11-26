import type { UpdateStatus } from '../types';
import { isInstalling, updateCard, versionOf } from '../logic/updates';

/**
 * The in-app update (SPEC-updates): Rust's status, read once at start and then followed through
 * `update:changed`, and Later, which lives only as long as this window does (until Airboss
 * starts again).
 */
export class UpdateStore {
  status = $state.raw<UpdateStatus>({ state: 'idle' });
  /** The version Later was chosen on: its offer or failure stays hidden. */
  later = $state<string | null>(null);
  /** A manual check is running (Settings' Check now says "Checking…"). */
  checking = $state(false);

  card = $derived(updateCard(this.status, this.later));

  set(s: UpdateStatus): void {
    this.status = s;
    // An install that starts (from the card or the toast) is shown, and a failure after it too.
    if (isInstalling(s)) this.later = null;
  }

  /** Later: hide the card for this version until Airboss starts again. */
  hide(): void {
    this.later = versionOf(this.status);
  }

  /** A manual check: the card shows again. */
  unhide(): void {
    this.later = null;
  }
}
