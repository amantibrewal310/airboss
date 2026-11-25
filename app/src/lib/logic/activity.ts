/*
  One line per history event: the actor, then a verb phrase per event kind. A subtask's events
  start with its id ("143 moved to Done").
*/

import type { HistoryEntry, Status, TaskId } from '../types';

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

function verb(e: HistoryEntry, self: boolean): Segment[] {
  const it = self ? 'this' : '';
  const sp = (s: string) => (it ? `${s} ${it}` : s);
  switch (e.kind) {
    case 'created':
      return [{ text: sp('created') }];
    case 'edited':
      return [{ text: e.body === 'title' ? 'renamed ' + (it || 'it') : e.body === 'notes' ? 'edited the notes' : e.body === 'branch' ? 'set the branch' : sp('edited') }];
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
    case 'linked':
      return e.actor === 'launch' && e.body?.startsWith('T3 · ')
        ? [{ text: 'started thread ' }, { text: e.body.slice(5), strong: true }]
        : [{ text: 'linked ' }, { text: e.body ?? 'a link', strong: true }];
    case 'unlinked':
      return [{ text: 'unlinked ' }, { text: e.body ?? 'a link', strong: true }];
    case 'link_state':
      return [{ text: 'saw the PR ' }, { text: e.body ?? 'change', strong: true }];
    case 'waits_on_added':
      return [{ text: 'made it wait on ' }, { text: e.body ?? '', strong: true }];
    case 'waits_on_removed':
      return [{ text: 'stopped it waiting on ' }, { text: e.body ?? '', strong: true }];
    case 'rejected':
      return [{ text: sp('rejected') }];
    case 'repo_linked':
      return [{ text: 'linked ' }, { text: e.body ?? 'a repo', strong: true }];
    case 'repo_unlinked':
      return [{ text: 'unlinked ' }, { text: e.body ?? 'a repo', strong: true }];
  }
}

/** The whole line, for the task `task` (its own events read "this"; a subtask's start with its id). */
export function phrase(e: HistoryEntry, task: TaskId): Segment[] {
  const self = e.task === task;
  const lead: Segment[] = self ? actorName(e) : [{ text: String(e.task), strong: true }];
  const v = verb(e, self);
  if (!self && e.kind !== 'status' && e.kind !== 'note') return [...lead, { text: ' · ' }, ...actorName(e), { text: ' ' }, ...v];
  if (!self && e.kind === 'note') return [...lead, { text: ' · ' }, ...actorName(e), ...v];
  return e.kind === 'note' ? [...lead, ...v] : [...lead, { text: ' ' }, ...v];
}
