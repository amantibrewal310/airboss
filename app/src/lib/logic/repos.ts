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

/** Add repos shows a search field once the list is longer than this. */
export const REPO_SEARCH_FROM = 8;

/**
 * Add repos' search: every word of the query appears in the repo's name or path, ignoring
 * case. Repos whose name holds every word come first; the rest match only on the path. Order
 * within each group is kept, so the list doesn't jump around as you type.
 */
export function filterRepos<T extends { name: string; path: string }>(items: T[], query: string): T[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return items;
  const byName: T[] = [];
  const byPath: T[] = [];
  for (const r of items) {
    const name = r.name.toLowerCase();
    const both = `${name} ${r.path.toLowerCase()}`;
    if (!words.every((w) => both.includes(w))) continue;
    (words.every((w) => name.includes(w)) ? byName : byPath).push(r);
  }
  return [...byName, ...byPath];
}

/**
 * Folders that could be added as repos (git, not added yet) whose name or path the task's
 * title or notes mention, such as "hub-ui: fix 7 tests" or "Repo: hub-ui (not registered in
 * Airboss)". Agents can't add repos, so they name an unregistered one in the text; Do it
 * offers to add it and start there. A name counts only as a whole word of 3+ characters.
 */
export function mentionedRepos<T extends { name: string; path: string; is_git: boolean; added: boolean }>(rows: T[], text: string): T[] {
  const lower = text.toLowerCase();
  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return rows.filter((r) => {
    if (r.added || !r.is_git) return false;
    const name = r.name.trim().toLowerCase();
    if (r.path && lower.includes(r.path.toLowerCase())) return true;
    return name.length >= 3 && new RegExp(`(^|[^a-z0-9_-])${esc(name)}($|[^a-z0-9_-])`).test(lower);
  });
}
