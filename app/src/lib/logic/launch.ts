/*
  Do it and Plan it results as toasts (SPEC-app "Do it and Plan it results"), built from
  `LaunchReport`. Each names the task and offers what can be done next.
*/

import type { LaunchReport, Mode, Repo, RepoId, RowDto, TaskId, ThreadKey } from '../types';

export type LaunchAction =
  | { kind: 'open_thread'; label: 'Open thread'; key: ThreadKey }
  | { kind: 'retry'; label: 'Retry' }
  | { kind: 'add_to_t3'; label: 'Add to T3 and retry'; repo: RepoId }
  | { kind: 'again'; label: 'Start again' };

export interface LaunchToast {
  text: string;
  tone: 'info' | 'warn';
  actions: LaunchAction[];
}

function names(ids: (RepoId | null)[], repos: Map<RepoId, Repo>): string[] {
  return ids.map((id) => (id === null ? 'research' : (repos.get(id)?.name ?? `repo ${id}`)));
}

function and(list: string[]): string {
  if (list.length <= 1) return list.join('');
  return `${list.slice(0, -1).join(', ')} and ${list[list.length - 1]}`;
}

export function launchToast(task: TaskId, mode: Mode, report: LaunchReport, repos: Map<RepoId, Repo>): LaunchToast {
  const started = report.started;
  const failed = report.failed;
  const open = (i: number): LaunchAction[] => (started[i] ? [{ kind: 'open_thread', label: 'Open thread', key: started[i]!.key }] : []);
  const retryFor = (): LaunchAction[] => {
    const notProject = failed.find(([repo, e]) => e.code === 'not_a_project' && repo !== null);
    return notProject ? [{ kind: 'add_to_t3', label: 'Add to T3 and retry', repo: notProject[0]! }] : [{ kind: 'retry', label: 'Retry' }];
  };

  if (started.length && failed.length) {
    const ok = and(names(started.map((s) => s.repo), repos));
    const bad = failed.map(([repo, e]) => `${names([repo], repos)[0]} failed: ${e.message}`).join(' ');
    return { text: `Started in ${ok}. ${bad}`, tone: 'warn', actions: retryFor() };
  }
  if (!started.length && failed.length) {
    return { text: `Couldn't start #${task}: ${failed[0]![1].message}`, tone: 'warn', actions: retryFor() };
  }
  if (!started.length) {
    const where = and(names(report.skipped, repos));
    return {
      text: where ? `#${task} already has threads in ${where}` : `#${task} already has a thread`,
      tone: 'info',
      actions: [{ kind: 'again', label: 'Start again' }],
    };
  }
  if (mode === 'plan_it') {
    return {
      text: `Started a planning thread for #${task}. Its steps become subtasks when you settle the plan.`,
      tone: 'info',
      actions: open(0),
    };
  }
  if (started.length === 1) {
    const s = started[0]!;
    if (s.repo === null) return { text: `Started a research thread for #${task}`, tone: 'info', actions: open(0) };
    return { text: `Started a thread for #${task} in ${names([s.repo], repos)[0]}`, tone: 'info', actions: open(0) };
  }
  const on = report.branch ? ` on ${report.branch}` : '';
  return { text: `Started ${started.length} threads for #${task}${on}`, tone: 'info', actions: [] };
}

/**
 * Start ready: the steps under `parent` that Do it can start now, in tree order. A step is a
 * leaf that is ready (todo, nothing it waits on is open). A branch whose own task waits on
 * something, or is closed, is skipped whole: a phase that waits on another holds its steps
 * back even though the steps themselves wait on nothing.
 */
export function readySteps(rows: Map<TaskId, RowDto>, order: TaskId[], parent: TaskId): TaskId[] {
  const kids = new Map<TaskId, TaskId[]>();
  for (const id of order) {
    const p = rows.get(id)?.parent;
    if (p != null) kids.set(p, [...(kids.get(p) ?? []), id]);
  }
  const out: TaskId[] = [];
  const walk = (id: TaskId) => {
    for (const c of kids.get(id) ?? []) {
      const r = rows.get(c);
      if (!r || r.waiting || r.status === 'done' || r.status === 'dropped' || r.status === 'proposed') continue;
      if (r.has_children) walk(c);
      else if (r.ready) out.push(c);
    }
  };
  walk(parent);
  return out;
}

/** What one Start ready did, step by step. */
export interface StartReadyResult {
  started: { task: TaskId; report: LaunchReport }[];
  /** Steps with no repo: Do it asks where, so they're left for the user to start. */
  noRepo: TaskId[];
  failed: { task: TaskId; message: string }[];
}

/** The one toast for a Start ready, in place of a toast per step. */
export function startReadyToast(parent: TaskId, r: StartReadyResult): { text: string; tone: 'info' | 'warn' } {
  const ids = (list: TaskId[]) => and(list.map((id) => `#${id}`));
  const ok = r.started.filter((s) => s.report.started.length > 0).map((s) => s.task);
  const parts: string[] = [];
  if (ok.length) parts.push(`Started ${ok.length === 1 ? 'step' : `${ok.length} steps`} of #${parent}: ${ids(ok)}.`);
  else if (!r.noRepo.length && !r.failed.length) parts.push(`Nothing new started for #${parent}.`);
  if (r.noRepo.length) parts.push(`${ids(r.noRepo)} ${r.noRepo.length === 1 ? 'has' : 'have'} no repo: open ${r.noRepo.length === 1 ? 'it' : 'each'} and press Do it to choose where.`);
  for (const f of r.failed) parts.push(`#${f.task} failed: ${f.message}`);
  return { text: parts.join(' '), tone: r.failed.length || r.noRepo.length ? 'warn' : 'info' };
}
