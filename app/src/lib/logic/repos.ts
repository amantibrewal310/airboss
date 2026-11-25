/*
  Settings › Repos: what removing a repo costs, for its confirmation (SPEC-app "Settings").
*/

import type { RepoId, RowDto, TaskId } from '../types';

/** Tasks linked to the repo themselves; a parent that only shows it from its subtasks isn't. */
export function tasksLinkedTo(rows: Map<TaskId, RowDto>, repo: RepoId): number {
  let n = 0;
  for (const r of rows.values()) if (!r.repos_from_subtasks && r.repos.includes(repo)) n++;
  return n;
}

/** "Remove booking-web? 12 tasks lose the link." */
export function removeQuestion(name: string, linked: number): string {
  if (linked === 0) return `Remove ${name}? No task is linked to it.`;
  return `Remove ${name}? ${linked} ${linked === 1 ? 'task loses' : 'tasks lose'} the link.`;
}
