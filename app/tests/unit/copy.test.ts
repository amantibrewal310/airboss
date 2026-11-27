import { describe, expect, it } from 'vitest';
import { groupFeed, launchSources, phrase } from '../../src/lib/logic/activity';
import { footerText, mergeConnections } from '../../src/lib/logic/connections';
import { headlineOf } from '../../src/lib/logic/needs';
import { sheetRows } from '../../src/lib/logic/commands';
import { statusToast } from '../../src/lib/logic/copy';
import type { Connection, HistoryEntry } from '../../src/lib/types';

/** P1-15: the words on screen. */
describe('copy', () => {
  it('toasts lead with the verb', () => {
    expect(statusToast(144, 'done')).toBe('Marked #144 done');
    expect(statusToast(144, 'blocked')).toBe('Marked #144 blocked');
    expect(statusToast(144, 'dropped')).toBe('Dropped #144');
    expect(statusToast(146, 'doing')).toBe('Moved #146 to In progress');
    expect(statusToast(146, 'todo')).toBe('Moved #146 to Todo');
    expect(statusToast(146, 'in_review')).toBe('Moved #146 to In review');
    // A small id on its own reads as a number word without the # (hands-on QA #5).
    expect(statusToast(1, 'done')).toBe('Marked #1 done');
  });

  it('a launch reads as the thread it started, and where', () => {
    // core writes a link event's body as "<kind> <ref>" (links.rs).
    const e: HistoryEntry = { id: 1, task: 144, kind: 'linked', to_status: null, body: 't3_thread slot-picker', actor: 'launch', at: '', thread_title: null };
    const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');
    expect(text(phrase(e, 144, { repoOf: (t) => (t === 'slot-picker' ? 'booking-web' : null) }))).toBe('Started thread slot-picker in booking-web');
    expect(text(phrase(e, 144))).toBe('Started thread slot-picker');
    expect(text(phrase({ ...e, task: 202 }, 200, { repoOf: () => 'booking-api' }))).toBe('202 · Started thread slot-picker in booking-api');
  });

  it('names a launched thread by its title, never its raw id (hands-on QA #11)', () => {
    const ref = 'mcp:9f5361a5-6809-48ba-9819-081f286d7495';
    const e: HistoryEntry = { id: 1, task: 1, kind: 'linked', to_status: null, body: `t3_thread ${ref}`, actor: 'launch', at: '', thread_title: null };
    const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');
    const repoOf = (t: string) => (t === ref ? 'scratch' : null);
    // The task's link knows the thread's title.
    expect(text(phrase(e, 1, { repoOf, threadTitle: (r) => (r === ref ? '1 QA one' : null) }))).toBe('Started thread 1 QA one in scratch');
    // Nothing knows it: "a thread", not the id.
    expect(text(phrase(e, 1, { repoOf }))).toBe('Started a thread in scratch');
    expect(text(phrase(e, 1))).toBe('Started a thread');
    expect(text(phrase({ ...e, body: 't3_thread 9f5361a5-6809-48ba-9819-081f286d7495' }, 1))).toBe('Started a thread');
    expect(text(phrase({ ...e, task: 2 }, 1))).toBe('2 · Started a thread');
    // The history's own title still counts.
    expect(text(phrase({ ...e, thread_title: 'slot-picker' }, 1))).toBe('Started thread slot-picker');
  });

  it('folds a plan import into one line per kind of event', () => {
    const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');
    const old = 'Imported by agent:t3/mcp:c55d from /Users/me/out/implementation-plan.md';
    const note = (id: number, task: number, body = old): HistoryEntry => ({ id, task, kind: 'note', to_status: null, body, actor: 'agent:t3/mcp:c55d', at: '', thread_title: 'Plan 3 Mira' });
    const created = (id: number, task: number): HistoryEntry => ({ id, task, kind: 'created', to_status: null, body: null, actor: 'plan', at: '', thread_title: null });
    const history = [note(10, 6), note(9, 5), note(8, 4), note(7, 7), created(4, 6), created(3, 5), note(2, 3, 'hi'), created(1, 3)];
    const lines = groupFeed(history, 3);
    expect(lines.map((l) => l.tasks)).toEqual([[6, 5, 4, 7], [6, 5], [], []]);
    expect(text(phrase(lines[0]!.entry, 3, {}, lines[0]!.tasks))).toBe('4 subtasks · Agent in Plan 3 Mira: Imported from implementation-plan.md');
    expect(text(phrase(lines[1]!.entry, 3, {}, lines[1]!.tasks))).toBe('5, 6 · Plan created');
    // The task's own events never fold.
    expect(groupFeed([note(2, 3, 'a'), note(1, 3, 'a')], 3).length).toBe(2);
    // The new note text reads as it is.
    expect(text(phrase(note(1, 3, 'Imported from plan.md'), 3))).toBe('Agent in Plan 3 Mira: Imported from plan.md');
  });

  it('never names an agent by a raw thread id', () => {
    const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');
    const e: HistoryEntry = { id: 1, task: 43, kind: 'created', to_status: null, body: null, actor: 'agent:t3/mcp:0e9396f8-fad7-4d4e-9db8-17052a62313e', at: '', thread_title: null };
    expect(text(phrase(e, 43))).toBe('An agent created this');
    expect(text(phrase({ ...e, thread_title: '4 Phase 1' }, 43))).toBe('Agent in 4 Phase 1 created this');
    expect(text(phrase({ ...e, actor: 'agent:slot-picker' }, 43))).toBe('Agent in slot-picker created this');
  });

  it('names Launch by where its thread runs: T3 or Herdr', () => {
    const ev = (id: number, over: Partial<HistoryEntry>): HistoryEntry => ({ id, task: 3, kind: 'status', to_status: 'doing', body: null, actor: 'launch', at: '', thread_title: null, ...over });
    const t3 = [ev(1, { actor: 'me', kind: 'created', to_status: null }), ev(2, { kind: 'linked', to_status: null, body: 't3_thread mcp:abc' }), ev(3, {})];
    const sources = launchSources(t3);
    expect(sources.get(2)).toBe('t3');
    expect(sources.get(3)).toBe('t3');
    expect(sources.has(1)).toBe(false);
    const text = (segs: { text: string }[]) => segs.map((s) => s.text).join('');
    const ctx = { launchedIn: (e: HistoryEntry) => sources.get(e.id) ?? null };
    expect(text(phrase(t3[2]!, 3, ctx))).toBe('T3 moved this to In progress');
    // Herdr; the status can come before the link in id order, and takes the link's source.
    const herdr = launchSources([ev(5, {}), ev(6, { kind: 'linked', to_status: null, body: 'herdr_terminal w1:p2' })]);
    expect(herdr.get(5)).toBe('herdr');
    expect(text(phrase(ev(5, {}), 3, { launchedIn: (e) => herdr.get(e.id) ?? null }))).toBe('Herdr moved this to In progress');
    // No thread link in the feed: still "Launch".
    expect(text(phrase(ev(7, {}), 3))).toBe('Launch moved this to In progress');
  });

  it('reads core’s event bodies in plain words', () => {
    const e = (x: Partial<HistoryEntry>): HistoryEntry => ({ id: 1, task: 144, kind: 'created', to_status: null, body: null, actor: 'me', at: '', thread_title: null, ...x });
    const text = (h: HistoryEntry, ctx = {}) => phrase(h, 144, ctx).map((s) => s.text).join('');
    expect(text(e({ kind: 'linked', body: 'pr acme/booking-api#58' }))).toBe('You linked PR #58');
    expect(text(e({ kind: 'linked', body: 'slack /archives/C0BOOKING/p1764000000' }), { linkTitle: () => '#booking thread' })).toBe('You linked #booking thread');
    expect(text(e({ kind: 'linked', body: 'doc ~/plans/booking-flow.md' }))).toBe('You linked booking-flow.md');
    expect(text(e({ kind: 'unlinked', body: 'slack /archives/C0BOOKING/p1764000000' }))).toBe('You unlinked Slack thread');
    expect(text(e({ kind: 'link_state', actor: 'sync:gh', body: 'acme/booking-api#58 merged' }))).toBe('GitHub saw PR #58 merged');
    expect(text(e({ kind: 'edited', body: 'title' }))).toBe('You renamed this');
    expect(text(e({ kind: 'edited', body: 'body' }))).toBe('You edited the notes');
    expect(text(e({ kind: 'edited', body: 'title, body' }))).toBe('You renamed this and edited the notes');
    expect(text(e({ kind: 'edited', body: 'branch: task-144-slot-picker' }))).toBe('You set the branch to task-144-slot-picker');
    expect(text(e({ kind: 'edited', body: 'branch cleared' }))).toBe('You cleared the branch');
    expect(text(e({ kind: 'waits_on_added', actor: 'plan', body: 'waits on #143, #145' }))).toBe('Plan made this wait on 143, 145');
    expect(text(e({ kind: 'waits_on_removed', body: 'no longer waits on #146' }))).toBe('You stopped this waiting on 146');
    expect(text(e({ kind: 'created', actor: 'plan', body: 'waits on #143' }))).toBe('Plan created this · waits on 143');
    expect(text(e({ kind: 'moved' }))).toBe('You moved this');
    expect(text(e({ kind: 'repo_linked', body: 'booking-web' }))).toBe('You linked booking-web');
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
    expect(headlineOf({ kind: 'quiet', headline: 'No activity for 6 days.' })).toBe('No activity for 6 days.');
  });

  it('the shortcut sheet uses plain words', () => {
    const labels = sheetRows('alt+space').map((r) => r.label);
    expect(labels).toContain('Close the page or panel');
    expect(labels.filter((l) => l.includes(' / '))).toEqual([]);
  });
});
