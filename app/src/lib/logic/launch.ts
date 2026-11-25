/*
  Do it and Plan it results as toasts (SPEC-app "Do it and Plan it results"), built from
  `LaunchReport`. Each names the task and offers what can be done next.
*/

import type { LaunchReport, Mode, Repo, RepoId, TaskId, ThreadKey } from '../types';

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
    return { text: `Couldn't start ${task}: ${failed[0]![1].message}`, tone: 'warn', actions: retryFor() };
  }
  if (!started.length) {
    const where = and(names(report.skipped, repos));
    return {
      text: where ? `${task} already has threads in ${where}` : `${task} already has a thread`,
      tone: 'info',
      actions: [{ kind: 'again', label: 'Start again' }],
    };
  }
  if (mode === 'plan_it') {
    return {
      text: `Started a planning thread for ${task}. Its steps become subtasks when you settle the plan.`,
      tone: 'info',
      actions: open(0),
    };
  }
  if (started.length === 1) {
    const s = started[0]!;
    if (s.repo === null) return { text: `Started a research thread for ${task}`, tone: 'info', actions: open(0) };
    return { text: `Started a thread for ${task} in ${names([s.repo], repos)[0]}`, tone: 'info', actions: open(0) };
  }
  const on = report.branch ? ` on ${report.branch}` : '';
  return { text: `Started ${started.length} threads for ${task}${on}`, tone: 'info', actions: [] };
}
