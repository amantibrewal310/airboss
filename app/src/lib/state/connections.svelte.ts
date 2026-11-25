import type { Availability, Connection } from '../types';
import { footerText, mergeConnections, t3Available, t3MissingReason } from '../logic/connections';

export class ConnectionsStore {
  rows = $state.raw<Connection[]>([]);
  availability = $state.raw<Availability>('unknown');
  /** "Database moved" or "Live updates stopped…", from `error:background`. */
  background = $state<string | null>(null);

  merged = $derived(mergeConnections(this.rows, this.availability));
  footer = $derived(footerText(this.merged, this.background));
  /** Do it and Plan it are on. Unknown counts as on until launch says otherwise. */
  t3Ready = $derived(this.availability === 'unknown' || t3Available(this.availability));
  t3Missing = $derived(t3MissingReason(this.availability));

  set(rows: Connection[], availability: Availability): void {
    this.rows = rows;
    this.availability = availability;
  }
}
