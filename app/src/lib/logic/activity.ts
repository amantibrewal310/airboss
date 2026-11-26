/*
  One line per history event: the actor, then a verb phrase per event kind. A subtask's events
  start with its id ("143 moved to Done").
*/

import type { EventId, HistoryEntry, LinkKind, Source, Status, TaskId } from '../types';
import { linkLabel } from './links';

export interface Segment {
  text: string;
  strong?: boolean;
}

const STATUS: Record<Status, string> = {
  proposed: 'Proposed',
  todo: 'Todo',
  doing: 'In progress',
  in_review: 'In review',
  blocked: 'Blocked',
  done: 'Done',
  dropped: 'Dropped',
};

/** Launch (Do it, Plan it) by where its thread runs: "T3", "Herdr", or "Launch" when unknown. */
export function launchName(source: Source | null | undefined): string {
  return source === 't3' ? 'T3' : source === 'herdr' ? 'Herdr' : 'Launch';
}

/** "You", "Agent in slot-picker", "GitHub", "T3", "Herdr", "Plan". */
export function actorName(e: Pick<HistoryEntry, 'actor' | 'thread_title'>, launchedIn?: Source | null): Segment[] {
  const a = e.actor;
  if (a === 'me') return [{ text: 'You' }];
  if (a.startsWith('agent:')) {
    // "agent:t3/mcp:0e93…": with no title known, the raw id reads as noise, so just "An agent".
    const raw = a.slice(6);
    const id = raw.includes('/') ? raw.slice(raw.indexOf('/') + 1) : raw;
    const ref = e.thread_title ?? (isRawThreadId(id) ? null : raw);
    return ref === null || ref === 'planner' ? [{ text: 'An agent' }] : [{ text: 'Agent in ' }, { text: ref, strong: true }];
  }
  if (a === 'sync:gh') return [{ text: 'GitHub' }];
  if (a === 'sync:t3') return [{ text: 'T3' }];
  if (a === 'plan') return [{ text: 'Plan' }];
  if (a === 'launch') return [{ text: launchName(launchedIn) }];
  return [{ text: a }];
}

export interface PhraseContext {
  /** The repo folder a thread runs in, by thread ref or title, for "Started thread X in booking-web". */
  repoOf?: (thread: string) => string | null;
  /** A linked item's title (the link tag's label) by kind and ref, when the task still has it. */
  linkTitle?: (kind: LinkKind, ref: string) => string | null;
  /** A T3 thread's or Herdr terminal's title by ref (its link's thread view or stored title), when the task has it. */
  threadTitle?: (ref: string) => string | null;
  /** Where a Launch event's thread runs, from the feed (see `launchSources`). */
  launchedIn?: (e: HistoryEntry) => Source | null;
}

/**
 * A thread ref that is an id ("mcp:9f5361a5-…", a bare UUID) rather than a name. Older and test
 * refs can be names ("slot-picker"); those read fine as they are.
 */
const isRawThreadId = (ref: string) => ref.includes(':') || /^[0-9a-f]{8}-?[0-9a-f]{4}/i.test(ref);

const LINK_KINDS = new Set<string>(['t3_thread', 'herdr_terminal', 'claude_session', 'pr', 'slack', 'doc', 'github_issue']);

/** core writes link events as "<kind> <ref>" (links.rs); anything else is shown as it is. */
export function parseLinkBody(body: string | null): { kind: LinkKind; ref: string } | null {
  if (!body) return null;
  const at = body.indexOf(' ');
  const kind = at > 0 ? body.slice(0, at) : '';
  return LINK_KINDS.has(kind) ? { kind: kind as LinkKind, ref: body.slice(at + 1) } : null;
}

function linkText(body: string | null, ctx: PhraseContext, fallback: string): string {
  const l = parseLinkBody(body);
  if (!l) return body ?? fallback;
  return ctx.linkTitle?.(l.kind, l.ref) ?? linkLabel({ kind: l.kind, ref: l.ref, title: null, url: null });
}

/** "#143, #145" → "143, 145". */
const ids = (s: string) => s.replace(/#(\d+)/g, '$1');

const LAUNCHED_KINDS = new Set<string>(['t3_thread', 'herdr_terminal']);
const isLaunchThread = (e: HistoryEntry) => e.kind === 'linked' && e.actor === 'launch' && LAUNCHED_KINDS.has(parseLinkBody(e.body)?.kind ?? '');

/**
 * Where each Launch event's thread runs, by event id. Do it and Plan it link the thread and move
 * the task in one go, so every Launch event on a task takes the source of that task's newest
 * Launch thread link at or before it (by event id).
 */
export function launchSources(history: HistoryEntry[]): Map<EventId, Source> {
  const out = new Map<EventId, Source>();
  const byTask = new Map<TaskId, HistoryEntry[]>();
  for (const e of history) if (e.actor === 'launch') byTask.set(e.task, [...(byTask.get(e.task) ?? []), e]);
  for (const events of byTask.values()) {
    const sorted = [...events].sort((a, b) => a.id - b.id);
    const sourceOf = (e: HistoryEntry): Source | null => {
      if (!isLaunchThread(e)) return null;
      return parseLinkBody(e.body)!.kind === 'herdr_terminal' ? 'herdr' : 't3';
    };
    // Each event's link is at or before it; an event with none before takes the first after.
    let current: Source | null = null;
    const first = sorted.map(sourceOf).find((s) => s !== null) ?? null;
    for (const e of sorted) {
      current = sourceOf(e) ?? current;
      const s = current ?? first;
      if (s) out.set(e.id, s);
    }
  }
  return out;
}

/**
 * A note as it reads in the feed. Imports before 0.1.0-alpha.6 wrote "Imported by agent:t3/<id>
 * from /full/path/plan.md"; the avatar already names the thread, so that reads as the new
 * "Imported from plan.md".
 */
function noteText(body: string): string {
  const old = /^Imported by \S+ from (.+)$/.exec(body);
  return old ? `Imported from ${old[1]!.replace(/\/+$/, '').split('/').pop()}` : body;
}

function edited(body: string | null, it: string): string {
  if (body?.startsWith('branch: ')) return `set the branch to ${body.slice(8)}`;
  if (body === 'branch cleared') return 'cleared the branch';
  const parts = new Set((body ?? '').split(',').map((p) => p.trim()));
  const said = [parts.has('title') ? `renamed ${it || 'it'}` : '', parts.has('body') || parts.has('notes') ? 'edited the notes' : ''].filter(Boolean);
  return said.length ? said.join(' and ') : it ? `edited ${it}` : 'edited';
}

function verb(e: HistoryEntry, self: boolean, ctx: PhraseContext): Segment[] {
  const it = self ? 'this' : '';
  const sp = (s: string) => (it ? `${s} ${it}` : s);
  switch (e.kind) {
    case 'created':
      return [{ text: sp('created') }, ...(e.body ? [{ text: ` · ${ids(e.body)}` }] : [])];
    case 'edited':
      return [{ text: edited(e.body, it) }];
    case 'moved':
      return [{ text: sp('moved') }];
    case 'status': {
      const to = e.to_status ? STATUS[e.to_status] : '';
      const segs: Segment[] = [{ text: self ? 'moved this to ' : 'moved to ' }, { text: to, strong: true }];
      if (e.body) segs.push({ text: ` · ${e.body}` });
      return segs;
    }
    case 'note':
      return [{ text: e.body ? `: ${noteText(e.body)}` : 'left a note' }];
    case 'linked': {
      if (!isLaunchThread(e)) return [{ text: 'linked ' }, { text: linkText(e.body, ctx, 'a link'), strong: true }];
      // The thread's title when anything knows it, else "a thread": never the raw id. The
      // stored history keeps the ref.
      const ref = parseLinkBody(e.body)!.ref;
      const thread = ctx.threadTitle?.(ref) ?? e.thread_title ?? (isRawThreadId(ref) ? null : ref);
      const repo = ctx.repoOf?.(ref) ?? (thread ? ctx.repoOf?.(thread) : null) ?? null;
      const where = repo ? [{ text: ' in ' }, { text: repo, strong: true }] : [];
      return thread ? [{ text: 'Started thread ' }, { text: thread, strong: true }, ...where] : [{ text: 'Started a thread' }, ...where];
    }
    case 'unlinked':
      return [{ text: 'unlinked ' }, { text: linkText(e.body, ctx, 'a link'), strong: true }];
    case 'link_state': {
      // "<pr ref> <state>", e.g. "acme/booking-api#58 merged".
      const at = e.body?.lastIndexOf(' ') ?? -1;
      if (!e.body || at < 0) return [{ text: 'saw the PR change' }];
      const pr = linkText(`pr ${e.body.slice(0, at)}`, ctx, 'the PR');
      return [{ text: 'saw ' }, { text: pr, strong: true }, { text: ` ${e.body.slice(at + 1)}` }];
    }
    case 'waits_on_added':
      return [{ text: `made ${it || 'it'} wait on ` }, { text: ids((e.body ?? '').replace(/^waits on /, '')), strong: true }];
    case 'waits_on_removed':
      return [{ text: `stopped ${it || 'it'} waiting on ` }, { text: ids((e.body ?? '').replace(/^no longer waits on /, '')), strong: true }];
    case 'rejected':
      return [{ text: sp('rejected') }, ...(e.body ? [{ text: ` · ${e.body}` }] : [])];
    case 'repo_linked':
      return [{ text: 'linked ' }, { text: e.body ?? 'a repo', strong: true }];
    case 'repo_unlinked':
      return [{ text: 'unlinked ' }, { text: e.body ?? 'a repo', strong: true }];
  }
}

/**
 * The whole line, for the task `task` (its own events read "this"; a subtask's start with its id).
 * A thread Launch started reads as the action, "Started thread slot-picker in booking-web"; the
 * avatar still says where it runs (T3 or Herdr).
 */
export function phrase(e: HistoryEntry, task: TaskId, ctx: PhraseContext = {}, group: TaskId[] = []): Segment[] {
  const self = e.task === task && group.length < 2;
  const launched = ctx.launchedIn?.(e) ?? null;
  const lead: Segment[] = self ? actorName(e, launched) : [{ text: group.length > 1 ? groupLabel(group) : String(e.task), strong: true }];
  const v = verb(e, self, ctx);
  if (isLaunchThread(e)) return self ? v : [...lead, { text: ' · ' }, ...v];
  if (!self && e.kind !== 'status' && e.kind !== 'note') return [...lead, { text: ' · ' }, ...actorName(e, launched), { text: ' ' }, ...v];
  if (!self && e.kind === 'note') return [...lead, { text: ' · ' }, ...actorName(e, launched), ...v];
  return e.kind === 'note' ? [...lead, ...v] : [...lead, { text: ' ' }, ...v];
}

/** A grouped line's subtasks: "4, 5, 6" for up to three, else "33 subtasks". */
function groupLabel(ids: TaskId[]): string {
  return ids.length <= 3 ? [...ids].sort((a, b) => a - b).join(', ') : `${ids.length} subtasks`;
}

/** One line of the feed: an event, and every subtask it happened to when the same thing happened to several in a row. */
export interface FeedLine {
  entry: HistoryEntry;
  /** The subtasks folded into this line, newest first; empty for a line about one task. */
  tasks: TaskId[];
}

/**
 * Folds runs of the same event on different subtasks into one line: a plan import writes a
 * created event, a note and sometimes a link on each of its tasks, which would otherwise be one
 * line per task. Only adjacent events fold, by the same actor, of the same kind, with the same
 * body and status, each on a different subtask (never the task itself).
 */
export function groupFeed(history: HistoryEntry[], task: TaskId): FeedLine[] {
  const out: FeedLine[] = [];
  const same = (a: HistoryEntry, b: HistoryEntry) =>
    a.actor === b.actor && a.kind === b.kind && a.body === b.body && a.to_status === b.to_status && a.thread_title === b.thread_title;
  for (const e of history) {
    const last = out[out.length - 1];
    if (e.task !== task && last && last.entry.task !== task && same(last.entry, e)) {
      const tasks = last.tasks.length ? last.tasks : [last.entry.task];
      if (!tasks.includes(e.task)) {
        last.tasks = [...tasks, e.task];
        continue;
      }
    }
    out.push({ entry: e, tasks: [] });
  }
  return out;
}
