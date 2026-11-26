import type { Availability, Connection, Timestamp } from '../types';
import { footerText, mergeConnections, t3Available, t3MissingReason } from '../logic/connections';
import { now as clockNow } from '../logic/clock';

export class ConnectionsStore {
  rows = $state.raw<Connection[]>([]);
  availability = $state.raw<Availability>('unknown');
  /** "Database moved" or "Live updates stopped…", from `error:background`. */
  background = $state<string | null>(null);
  /** Whether the `airboss` command-line tool is installed; the Agents row needs it. */
  cliInstalled = $state(true);
  /** When an agent last reported, for the Agents row's "Last report 3m ago". */
  agentReport = $state<Timestamp | null>(null);
  /** The minute tick, so that time stays current. */
  now = $state(clockNow());

  merged = $derived(
    mergeConnections(this.rows, this.availability, { cliInstalled: this.cliInstalled, agentReport: this.agentReport, now: this.now }),
  );
  footer = $derived(footerText(this.merged, this.background));
  /** Do it and Plan it are on. Unknown counts as on until launch says otherwise. */
  t3Ready = $derived(this.availability === 'unknown' || t3Available(this.availability));
  t3Missing = $derived(t3MissingReason(this.availability));

  set(rows: Connection[], availability: Availability, agentReport: Timestamp | null = this.agentReport): void {
    this.rows = rows;
    this.availability = availability;
    this.agentReport = agentReport;
  }
}
