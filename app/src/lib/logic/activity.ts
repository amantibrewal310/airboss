/*
  One line per history event: the actor, then a verb phrase per event kind. A subtask's events
  start with its id ("143 moved to Done").
*/

import type { HistoryEntry, LinkKind, Status, TaskId } from '../types';
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

/** "You", "Agent in slot-picker", "GitHub", "T3", "Plan", "Launch". */
export function actorName(e: Pick<HistoryEntry, 'actor' | 'thread_title'>): Segment[] {
  const a = e.actor;
  if (a === 'me') return [{ text: 'You' }];
  if (a.startsWith('agent:')) {
    const ref = e.thread_title ?? a.slice(6);
    return ref === 'planner' ? [{ text: 'An agent' }] : [{ text: 'Agent in ' }, { text: ref, strong: true }];
  }
  if (a === 'sync:gh') return [{ text: 'GitHub' }];
  if (a === 'sync:t3') return [{ text: 'T3' }];
  if (a === 'plan') return [{ text: 'Plan' }];
  if (a === 'launch') return [{ text: 'Launch' }];
  return [{ text: a }];
}

export interface PhraseContext {
  /** The repo folder a thread runs in, by thread ref or title, for "Started thread X in booking-web". */
  repoOf?: (thread: string) => string | null;
  /** A linked item's title (the link tag's label) by kind and ref, when the task still has it. */
  linkTitle?: (kind: LinkKind, ref: string) => string | null;
  /** A T3 thread's title by ref (its link's thread view or stored title), when the task has it. */
  threadTitle?: (ref: string) => string | null;
}

/**
 * A thread ref that is an id ("mcp:9f5361a5-…", a bare UUID) rather than a name. Older and test
 * refs can be names ("slot-picker"); those read fine as they are.
 */
const isRawThreadId = (ref: string) => ref.includes(':') || /^[0-9a-f]{8}-?[0-9a-f]{4}/i.test(ref);

const LINK_KINDS = new Set<string>(['t3_thread', 'claude_session', 'pr', 'slack', 'doc', 'github_issue']);

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

const isLaunchThread = (e: HistoryEntry) => e.kind === 'linked' && e.actor === 'launch' && parseLinkBody(e.body)?.kind === 't3_thread';

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
      return [{ text: e.body ? `: ${e.body}` : 'left a note' }];
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
 * avatar still says it was Launch.
 */
export function phrase(e: HistoryEntry, task: TaskId, ctx: PhraseContext = {}): Segment[] {
  const self = e.task === task;
  const lead: Segment[] = self ? actorName(e) : [{ text: String(e.task), strong: true }];
  const v = verb(e, self, ctx);
  if (isLaunchThread(e)) return self ? v : [...lead, { text: ' · ' }, ...v];
  if (!self && e.kind !== 'status' && e.kind !== 'note') return [...lead, { text: ' · ' }, ...actorName(e), { text: ' ' }, ...v];
  if (!self && e.kind === 'note') return [...lead, { text: ' · ' }, ...actorName(e), ...v];
  return e.kind === 'note' ? [...lead, ...v] : [...lead, { text: ' ' }, ...v];
}
