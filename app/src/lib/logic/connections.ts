import type { Availability, Connection } from '../types';

const ORDER = ['T3 Code', 'GitHub', 'Agents', 'Claude Code'];

export function t3Available(a: Availability): boolean {
  return typeof a === 'object' && 'ready' in a;
}

export function t3MissingReason(a: Availability): string | null {
  return typeof a === 'object' && 'missing' in a ? a.missing.reason : null;
}

/**
 * Rows with the same name are one row: ok when all are ok; state, detail and fix from the
 * first that isn't (EM decision, 26 Nov). T3 Code is also off when `launch` says it's missing.
 * Claude Code shows only when it's broken.
 */
export function mergeConnections(rows: Connection[], availability: Availability, app: { cliInstalled?: boolean } = {}): Connection[] {
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
    byName.set('Agents', { ...agents, ok: false, state: 'Not set up', fix: 'Install the command-line tool, then copy the agent note into each repo.' });
  }
  const missing = t3MissingReason(availability);
  const t3 = byName.get('T3 Code');
  if (missing && (!t3 || t3.ok)) {
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

/** The footer: "All connected", the first problem in plain words, or "N connections need setup". */
export function footerText(merged: Connection[], background: string | null): { ok: boolean; text: string } {
  if (background) return { ok: false, text: background };
  const off = merged.filter((c) => !c.ok);
  if (!off.length) return { ok: true, text: 'All connected' };
  if (off.length === 1) return { ok: false, text: `${off[0]!.name} ${off[0]!.state.toLowerCase()}` };
  return { ok: false, text: `${off.length} connections need setup` };
}
