import type { AgentTools, Availability, Connection, HerdrAgent, LauncherKind, Timestamp } from '../types';
import { agentsStep, codexNeedsTerminalId, footerText, LAUNCHER_NAME, launcherMissingText, mergeConnections, t3Available, t3MissingReason } from '../logic/connections';
import { now as clockNow } from '../logic/clock';

export class ConnectionsStore {
  rows = $state.raw<Connection[]>([]);
  availability = $state.raw<Availability>('unknown');
  /** Whether Herdr is there to start agents in. */
  herdr = $state.raw<Availability>('unknown');
  /** Where Do it and Plan it start agents now ("Start agents in", resolved by Rust). */
  launcher = $state<LauncherKind>('t3');
  /** "Database moved" or "Live updates stopped…", from `error:background`. */
  background = $state<string | null>(null);
  /** Whether the `airboss` command-line tool is installed; the Agents row needs it. */
  cliInstalled = $state(true);
  /** Which agent tools have the airboss MCP server; Agents are Ready only with one. */
  agents = $state.raw<AgentTools>({ claude: false, codex: false, codex_terminal_id: false });
  /** The agent a Herdr tab runs (Settings › Agent in Herdr), kept in step by the app. */
  herdrAgent = $state<HerdrAgent>('claude');
  /** Connect Claude Code is running (it can take a few seconds). */
  connecting = $state(false);
  /** When an agent last reported, for the Agents row's "Last report 3m ago". */
  agentReport = $state<Timestamp | null>(null);
  /** The minute tick, so that time stays current. */
  now = $state(clockNow());

  merged = $derived(
    mergeConnections(this.rows, this.availability, {
      cliInstalled: this.cliInstalled,
      agents: this.agents,
      agentReport: this.agentReport,
      now: this.now,
      launcher: this.launcher,
    }),
  );
  /** First run's Agents button: Install command-line tool, Connect Claude Code, or Copy agent note. */
  agentsStep = $derived(agentsStep(this.cliInstalled, this.agents, this.codexOutdated));
  footer = $derived(footerText(this.merged, this.background));
  /** Codex runs in Herdr with a setup that lacks HC_TERMINAL_ID: "Copy Codex setup" fixes it. */
  get codexOutdated(): boolean {
    return codexNeedsTerminalId(this.agents, this.launcher, this.herdrAgent);
  }
  /** T3 Code can start threads (for its projects in Add repos). Unknown counts as on. */
  t3Ready = $derived(this.availability === 'unknown' || t3Available(this.availability));
  /** "T3 Code", "Herdr": where Do it and Plan it start agents, for copy. */
  launcherName = $derived(LAUNCHER_NAME[this.launcher]);
  private launchAvailability = $derived(this.launcher === 'herdr' ? this.herdr : this.availability);
  /** Do it and Plan it are on: the launcher in use is there. Unknown counts as on until launch says otherwise. */
  launchReady = $derived(this.launchAvailability === 'unknown' || t3Available(this.launchAvailability));
  /** Why agents can't start, naming the launcher ("Starting agents needs Herdr"). */
  launchMissing = $derived(this.launchReady ? null : (t3MissingReason(this.launchAvailability) ?? launcherMissingText(this.launcher)));
  /** The disabled buttons' short reason, naming the launcher: "Herdr isn't installed". */
  launchOff = $derived(`Starting agents needs ${this.launcherName}`);

  set(
    rows: Connection[],
    availability: Availability,
    agentReport: Timestamp | null = this.agentReport,
    launch: { herdr: Availability; launcher: LauncherKind } = { herdr: this.herdr, launcher: this.launcher },
  ): void {
    this.rows = rows;
    this.availability = availability;
    this.agentReport = agentReport;
    this.herdr = launch.herdr;
    this.launcher = launch.launcher;
  }
}
