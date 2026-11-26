/*
  Toast wording, verb first: "Marked #144 done", "Moved #146 to In progress", "Dropped #144".
  A task id on its own in a sentence is written "#144", as the CLI does, so "Marked #1 done"
  doesn't read as "marked one done". Rows and lists keep the bare id.
  Also the short sentences beside a thread ("stopped 6m ago").
*/

import type { Status, TaskId, ThreadDto } from '../types';
import { ago, relative } from './time';
import { SHAPE_LABEL } from './views';

export function statusToast(id: TaskId, status: Status): string {
  if (status === 'done' || status === 'blocked') return `Marked #${id} ${status}`;
  if (status === 'dropped') return `Dropped #${id}`;
  return `Moved #${id} to ${SHAPE_LABEL[status]}`;
}

/** A linked thread's line in the panel: "asked a question just now", "working · 6m", "stopped 2h ago". */
export function threadSummary(t: ThreadDto, now: number): string {
  const at = t.request?.at ?? t.last_activity_at;
  if (t.state === 'needs_me' && t.request) return `${t.request.kind === 'approval' ? 'wants approval' : 'asked a question'} ${ago(at, now)}`;
  if (t.state === 'errored') return t.error ?? 'stopped with an error';
  if (t.state === 'running') return `working · ${relative(at, now)}`;
  if (t.state === 'gone') return t.key.source === 'herdr' ? 'no longer in Herdr' : 'no longer in T3';
  return `stopped ${ago(at, now)}`;
}
