import type { AgentTools, Availability, Connection, HerdrAgent, LauncherKind, Timestamp } from '../types';
import { ago } from './time';

const ORDER = ['T3 Code', 'Herdr', 'GitHub', 'Agents', 'Claude Code'];

/** Where Do it and Plan it start agents, as copy names it. */
export const LAUNCHER_NAME: Record<LauncherKind, string> = { t3: 'T3 Code', herdr: 'Herdr' };

/** "Do it and Plan it need Herdr. Install it, then choose Check again." */
export function launcherMissingText(launcher: LauncherKind): string {
  return `Do it and Plan it need ${LAUNCHER_NAME[launcher]}. Install it, then choose Check again.`;
}

export function t3Available(a: Availability): boolean {
  return typeof a === 'object' && 'ready' in a;
}

export function t3MissingReason(a: Availability): string | null {
  return typeof a === 'object' && 'missing' in a ? a.missing.reason : null;
}

/**
 * Rows with the same name are one row: ok when all are ok; state, detail and fix from the
 * first that isn't (EM decision, 26 Nov). T3 Code is also off when `launch` says it's missing,
 * unless agents start in Herdr (Rust leaves that row out then, and it isn't put back).
 * Claude Code shows only when it's broken.
 */
export function mergeConnections(
  rows: Connection[],
  availability: Availability,
  app: { cliInstalled?: boolean; agents?: AgentTools; agentReport?: Timestamp | null; now?: number; launcher?: LauncherKind } = {},
): Connection[] {
  const byName = new Map<string, Connection>();
  for (const r of rows) {
    const prev = byName.get(r.name);
    if (!prev) byName.set(r.name, { ...r });
    else if (prev.ok && !r.ok) byName.set(r.name, { ...r });
  }
  // Agents can't report without the command-line tool, so the row says so (and the footer agrees)
  // until it's installed, whatever the last report was.
  const agents = byName.get('Agents');
  if (app.cliInstalled === false && agents?.ok) {
    byName.set('Agents', { ...agents, ok: false, state: 'Not set up', fix: 'Install the command-line tool, then connect Claude Code.' });
  }
  // The CLI alone isn't enough (#9): an agent tool needs the airboss MCP server too.
  const wired = byName.get('Agents');
  if (app.cliInstalled !== false && app.agents && !anyConnected(app.agents) && wired?.ok) {
    byName.set('Agents', { ...wired, ...NOT_CONNECTED });
  }
  // A Ready Agents row says when an agent last reported (SPEC-app: the CLI is installed, and
  // the time of the last report), or that none has: Ready alone doesn't mean one is set up.
  const ready = byName.get('Agents');
  if (ready?.ok && 'agentReport' in app) {
    byName.set('Agents', { ...ready, detail: lastReport(app.agentReport ?? null, app.now ?? Date.now()) });
  }
  const missing = t3MissingReason(availability);
  const t3 = byName.get('T3 Code');
  if (missing && (t3 ? t3.ok : app.launcher !== 'herdr')) {
    byName.set('T3 Code', {
      name: 'T3 Code',
      ok: false,
      state: 'Not found',
      detail: t3?.detail ?? 'Do it and Plan it start agent threads here.',
      fix: 'Install T3 Code to turn on Do it and Plan it. Everything else works.',
    });
  }
  const out = [...byName.values()].filter((c) => !(c.name === 'Claude Code' && c.ok));
  const rank = (n: string) => (ORDER.indexOf(n) === -1 ? ORDER.length : ORDER.indexOf(n));
  return out.sort((a, b) => rank(a.name) - rank(b.name));
}

/** The Agents row with the CLI but no agent tool that has the server, as Rust words it. */
const NOT_CONNECTED = {
  ok: false,
  state: 'Not connected',
  detail: "Agents can't report to Airboss yet.",
  fix: "Agents can't report to Airboss yet. Connect Claude Code in Settings › Agent setup.",
};

export function anyConnected(a: AgentTools): boolean {
  return a.claude || a.codex;
}

/**
 * Codex runs in Herdr with an airboss table from before Herdr: without `HC_TERMINAL_ID` its
 * agents can't say which terminal they are (Rust turns the Agents row orange for it).
 */
export function codexNeedsTerminalId(agents: AgentTools, launcher: LauncherKind, agent: HerdrAgent): boolean {
  return launcher === 'herdr' && agent === 'codex' && agents.codex && !agents.codex_terminal_id;
}

/**
 * The Agents row's one next step on first run: install the CLI (missing or broken), connect
 * Claude Code (no agent tool has the server), copy the Codex setup again (Codex in Herdr
 * with a setup that lacks `HC_TERMINAL_ID`), or copy the agent note (ready).
 */
export type AgentsStep = 'install' | 'connect' | 'codex' | 'note';

export function agentsStep(cliInstalled: boolean, agents: AgentTools, codexOutdated = false): AgentsStep {
  if (!cliInstalled) return 'install';
  if (codexOutdated) return 'codex';
  return anyConnected(agents) ? 'note' : 'connect';
}

/** The footer: "All connected", the first problem in plain words, or "N connections need setup". */
export function footerText(merged: Connection[], background: string | null): { ok: boolean; text: string } {
  if (background) return { ok: false, text: background };
  const off = merged.filter((c) => !c.ok);
  if (!off.length) return { ok: true, text: 'All connected' };
  if (off.length === 1) return { ok: false, text: `${off[0]!.name} ${off[0]!.state.toLowerCase()}` };
  return { ok: false, text: `${off.length} connections need setup` };
}

/** "Last report 3m ago", "Last report just now", "Last report on 4 Nov", or none yet. */
export function lastReport(at: Timestamp | null, now: number): string {
  if (!at) return 'No agent has reported yet.';
  return `Last report ${ago(at, now)}`;
}
