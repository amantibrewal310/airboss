import { describe, expect, it } from 'vitest';
import { applyPatch, type RowsState } from '../../src/lib/logic/patch';
import { ago as agoText, clockTime, feedTime, relative, relativeLong } from '../../src/lib/logic/time';
import { needsLead } from '../../src/lib/logic/needs';
import { threadSummary } from '../../src/lib/logic/copy';
import type { ThreadDto } from '../../src/lib/types';
import { agentsStep, footerText, mergeConnections } from '../../src/lib/logic/connections';
import type { Connection, RowDto } from '../../src/lib/types';
import { lastReport } from '../../src/lib/logic/connections';

const row = (id: number, extra: Partial<RowDto> = {}): RowDto => ({
  id, parent: null, depth: 0, position: id, title: `T${id}`, status: 'todo', origin: 'me', blocked_reason: null,
  waiting: false, waits_on: [], has_children: false, progress: null, repos: [], repos_from_subtasks: false,
  branch: null, ready: false, ready_since: null, ready_pill: false, quiet_days: null, needs: false,
  needs_stale: false, live: null, last_at: '2026-11-25T10:00:00.000Z', last_actor: 'me', ...extra,
});

describe('applyPatch', () => {
  const base = (): RowsState => ({ rows: new Map([[1, row(1)], [2, row(2)], [3, row(3)]]), order: [1, 2, 3], repos: [], seq: 5 });

  it('ignores a patch whose seq was already applied', () => {
    expect(applyPatch(base(), { seq: 5, upserts: [row(1, { title: 'x' })], removed: [], order: null, repos: null, created: [] })).toBeNull();
    expect(applyPatch(base(), { seq: 4, upserts: [], removed: [], order: null, repos: null, created: [] })).toBeNull();
  });

  it('ignores an empty patch (seq 0), even with nothing applied yet, and keeps the seq', () => {
    const empty = { seq: 0, upserts: [], removed: [], order: null, repos: null, created: [] };
    expect(applyPatch(base(), empty)).toBeNull();
    expect(applyPatch({ ...base(), seq: 0 }, empty)).toBeNull();
    // A real patch after it still applies.
    expect(applyPatch(base(), { ...empty, seq: 6 })!.seq).toBe(6);
  });

  it('replaces only changed rows, keeping the others’ identity', () => {
    const s = base();
    const next = applyPatch(s, { seq: 6, upserts: [row(2, { title: 'changed' })], removed: [], order: null, repos: null, created: [] })!;
    expect(next.rows.get(2)!.title).toBe('changed');
    expect(next.rows.get(1)).toBe(s.rows.get(1));
    expect(next.rows).not.toBe(s.rows);
    expect(next.order).toBe(s.order);
    expect(next.seq).toBe(6);
  });

  it('takes order and repos only when sent, and drops removed rows', () => {
    const s = base();
    const next = applyPatch(s, { seq: 6, upserts: [], removed: [2], order: null, repos: null, created: [] })!;
    expect(next.order).toEqual([1, 3]);
    expect(next.rows.has(2)).toBe(false);
    const reordered = applyPatch(next, { seq: 7, upserts: [], removed: [], order: [3, 1], repos: [{ id: 1, name: 'a', path: '/a', remote_url: null }], created: [] })!;
    expect(reordered.order).toEqual([3, 1]);
    expect(reordered.repos).toHaveLength(1);
  });
});

describe('time', () => {
  const now = Date.parse('2026-11-25T10:44:00.000Z');
  const ago = (ms: number) => new Date(now - ms).toISOString();
  it('writes row times short', () => {
    expect(relative(ago(20_000), now)).toBe('now');
    expect(relative(ago(6 * 60_000), now)).toBe('6m');
    expect(relative(ago(3 * 3_600_000), now)).toBe('3h');
    expect(relative(ago(2 * 86_400_000), now)).toBe('2d');
    expect(relative(ago(15 * 86_400_000), now)).toBe('2w');
    expect(relative(ago(90 * 86_400_000), now)).toBe('27 Aug');
    expect(relativeLong(ago(6 * 60_000), now)).toBe('6 minutes ago');
  });
  it('writes "ago" phrases that never read "now ago" (hands-on QA #17)', () => {
    expect(agoText(ago(20_000), now)).toBe('just now');
    expect(agoText(ago(6 * 60_000), now)).toBe('6m ago');
    expect(agoText(ago(3 * 3_600_000), now)).toBe('3h ago');
    expect(agoText(ago(2 * 86_400_000), now)).toBe('2d ago');
    expect(agoText(ago(15 * 86_400_000), now)).toBe('2w ago');
    expect(agoText(ago(90 * 86_400_000), now)).toBe('on 27 Aug');
    expect(agoText(null, now)).toBe('');
    // A clock a little ahead of the stamp is still "just now".
    expect(agoText(new Date(now + 5_000).toISOString(), now)).toBe('just now');
  });
  it('builds the Needs card lead and thread lines from it', () => {
    expect(needsLead({ kind: 'question', since: ago(20_000) }, now)).toBe('Question · just now');
    expect(needsLead({ kind: 'agent_stopped', since: ago(12 * 60_000) }, now)).toBe('Agent stopped · 12m ago');
    const t = (over: Partial<ThreadDto>): ThreadDto =>
      ({ key: { source: 't3', id: 'x' }, state: 'idle', chip: null, title: null, request: null, error: null, project_path: null, url: null, last_activity_at: ago(20_000), ...over }) as ThreadDto;
    expect(threadSummary(t({}), now)).toBe('stopped just now');
    expect(threadSummary(t({ last_activity_at: ago(6 * 60_000) }), now)).toBe('stopped 6m ago');
    expect(threadSummary(t({ state: 'running' }), now)).toBe('working · now');
    expect(threadSummary(t({ state: 'needs_me', request: { kind: 'question', at: ago(20_000) } as ThreadDto['request'] }), now)).toBe('asked a question just now');
    expect(threadSummary(t({ state: 'needs_me', request: { kind: 'approval', at: ago(3 * 3_600_000) } as ThreadDto['request'] }), now)).toBe('wants approval 3h ago');
    expect(threadSummary(t({ state: 'errored', error: 'boom' }), now)).toBe('boom');
    expect(threadSummary(t({ state: 'gone' }), now)).toBe('no longer in T3');
    expect(lastReport(ago(20_000), now)).toBe('Last report just now');
    expect(lastReport(ago(6 * 60_000), now)).toBe('Last report 6m ago');
    expect(lastReport(ago(90 * 86_400_000), now)).toBe('Last report on 27 Aug');
  });
  it('writes feed times as relative today, the weekday this week, else the date', () => {
    expect(feedTime(ago(6 * 60_000), now)).toBe('6m');
    expect(feedTime('2026-11-23T09:00:00.000Z', now)).toBe('Mon');
    expect(feedTime('2026-11-02T09:00:00.000Z', now)).toBe('2 Nov');
  });
  it('writes clock times for the native menu', () => {
    const today = new Date(now - 12 * 60_000);
    expect(clockTime(today.toISOString(), now)).toBe(today.toTimeString().slice(0, 5));
    const mon = new Date('2026-11-23T10:32:00.000Z');
    expect(clockTime(mon.toISOString(), now)).toBe(`Mon ${mon.toTimeString().slice(0, 5)}`);
    expect(clockTime('2026-11-10T10:32:00.000Z', now)).toBe('10 Nov');
  });
});

describe('connections', () => {
  const c = (name: string, ok: boolean, state: string): Connection => ({ name, ok, state, detail: `${name} detail`, fix: ok ? null : 'fix it' });
  it('merges rows with the same name, the worst state winning', () => {
    const merged = mergeConnections([c('T3 Code', true, 'Connected'), c('GitHub', true, 'Signed in'), c('T3 Code', false, 'Not updating since 14:02')], 'unknown');
    expect(merged.map((m) => [m.name, m.ok, m.state])).toEqual([
      ['T3 Code', false, 'Not updating since 14:02'],
      ['GitHub', true, 'Signed in'],
    ]);
  });
  it('turns T3 Code off when launch says it is missing, and hides a healthy Claude Code row', () => {
    const merged = mergeConnections([c('T3 Code', true, 'Connected'), c('Claude Code', true, 'Reading sessions')], { missing: { reason: 'x' } });
    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ name: 'T3 Code', ok: false, state: 'Not found' });
  });
  it('says when an agent last reported on a Ready Agents row, or that none has', () => {
    const now = Date.parse('2026-11-25T10:44:00.000Z');
    const ready = c('Agents', true, 'Ready');
    const at = (ts: string | null) => mergeConnections([ready], 'unknown', { agentReport: ts, now })[0]!.detail;
    expect(at('2026-11-25T10:41:00.000Z')).toBe('Last report 3m ago');
    expect(at('2026-11-25T10:43:40.000Z')).toBe('Last report just now');
    expect(at('2026-11-23T09:00:00.000Z')).toBe('Last report 2d ago');
    expect(at(null)).toBe('No agent has reported yet.');
    // A row that isn't Ready keeps its own words.
    const broken = mergeConnections([c('Agents', false, 'Link broken')], 'unknown', { agentReport: '2026-11-25T10:41:00.000Z', now });
    expect(broken[0]!.detail).toBe('Agents detail');
  });
  it('is Not connected with the CLI but no agent tool that has the server (#9)', () => {
    const now = Date.parse('2026-11-25T10:44:00.000Z');
    const ready = c('Agents', true, 'Ready');
    const none = { claude: false, codex: false, codex_terminal_id: false };
    const off = mergeConnections([ready], 'unknown', { cliInstalled: true, agents: none, agentReport: null, now })[0]!;
    expect(off).toMatchObject({ ok: false, state: 'Not connected', detail: "Agents can't report to Airboss yet." });
    expect(footerText([off], null)).toEqual({ ok: false, text: 'Agents not connected' });
    // Without the CLI, that comes first.
    expect(mergeConnections([ready], 'unknown', { cliInstalled: false, agents: none })[0]!.state).toBe('Not set up');
    // Either tool is enough, and Ready keeps its last-report line.
    for (const agents of [{ claude: true, codex: false, codex_terminal_id: false }, { claude: false, codex: true, codex_terminal_id: true }]) {
      const on = mergeConnections([ready], 'unknown', { cliInstalled: true, agents, agentReport: '2026-11-25T10:41:00.000Z', now })[0]!;
      expect(on).toMatchObject({ ok: true, state: 'Ready', detail: 'Last report 3m ago' });
    }
  });
  it('picks first run’s one Agents step', () => {
    expect(agentsStep(false, { claude: true, codex: true, codex_terminal_id: true })).toBe('install');
    expect(agentsStep(true, { claude: false, codex: false, codex_terminal_id: false })).toBe('connect');
    expect(agentsStep(true, { claude: false, codex: true, codex_terminal_id: true })).toBe('note');
    expect(agentsStep(true, { claude: true, codex: false, codex_terminal_id: false })).toBe('note');
  });
  it('names the first problem, or counts them', () => {
    expect(footerText([c('GitHub', true, 'Signed in')], null)).toEqual({ ok: true, text: 'All connected' });
    expect(footerText([c('T3 Code', false, 'Not found')], null)).toEqual({ ok: false, text: 'T3 Code not found' });
    expect(footerText([c('T3 Code', false, 'Not found'), c('GitHub', false, 'Not signed in')], null).text).toBe('2 connections need setup');
    expect(footerText([], 'Database moved').text).toBe('Database moved');
    // attention's own row when its table can't open.
    expect(footerText(mergeConnections([c('Needs me', false, "Didn't start")], 'unknown'), null).text).toBe("Needs me didn't start");
  });
});
