import { describe, expect, it } from 'vitest';
import { phrase } from '../../src/lib/logic/activity';
import { footerText, mergeConnections } from '../../src/lib/logic/connections';
import { headlineOf } from '../../src/lib/logic/needs';
import { sheetRows } from '../../src/lib/logic/commands';
import { statusToast } from '../../src/lib/logic/copy';
import type { Connection, HistoryEntry } from '../../src/lib/types';

/** P1-15: the words on screen. */
describe('copy', () => {
  it('toasts lead with the verb', () => {
    expect(statusToast(144, 'done')).toBe('Marked 144 done');
    expect(statusToast(144, 'blocked')).toBe('Marked 144 blocked');
    expect(statusToast(144, 'dropped')).toBe('Dropped 144');
    expect(statusToast(146, 'doing')).toBe('Moved 146 to In progress');
    expect(statusToast(146, 'todo')).toBe('Moved 146 to Todo');
    expect(statusToast(146, 'in_review')).toBe('Moved 146 to In review');
  });

  it('a launch reads as the thread it started, and where', () => {
    const e: HistoryEntry = { id: 1, task: 144, kind: 'linked', to_status: null, body: 'T3 · slot-picker', actor: 'launch', at: '', thread_title: null };
    const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');
    expect(text(phrase(e, 144, { repoOf: (t) => (t === 'slot-picker' ? 'booking-web' : null) }))).toBe('Started thread slot-picker in booking-web');
    expect(text(phrase(e, 144))).toBe('Started thread slot-picker');
    expect(text(phrase({ ...e, task: 202 }, 200, { repoOf: () => 'booking-api' }))).toBe('202 · Started thread slot-picker in booking-api');
  });

  it('the Agents row and the footer agree with the command-line tool', () => {
    const agents: Connection = { name: 'Agents', ok: true, state: 'Ready', detail: 'Agents report progress and ask questions through Airboss.', fix: null };
    const merged = mergeConnections([agents], 'unknown', { cliInstalled: false });
    expect(merged[0]).toMatchObject({ name: 'Agents', ok: false, state: 'Not set up' });
    expect(merged[0]!.fix).toContain('Install the command-line tool');
    expect(footerText(merged, null)).toEqual({ ok: false, text: 'Agents not set up' });
    expect(mergeConnections([agents], 'unknown', { cliInstalled: true })[0]).toMatchObject({ ok: true, state: 'Ready' });
  });

  it('a Needs me headline does not repeat the kind it sits under', () => {
    expect(headlineOf({ kind: 'ready', headline: 'Ready to start: 143 is done' })).toBe('143 is done');
    expect(headlineOf({ kind: 'question', headline: 'Use the existing date picker?' })).toBe('Use the existing date picker?');
    expect(headlineOf({ kind: 'finish_it', headline: 'All 4 subtasks are done. Mark the parent done?' })).toBe('All 4 subtasks are done. Mark the parent done?');
  });

  it('the shortcut sheet uses plain words', () => {
    const labels = sheetRows('alt+space').map((r) => r.label);
    expect(labels).toContain('Close the page or panel');
    expect(labels.filter((l) => l.includes(' / '))).toEqual([]);
  });
});
