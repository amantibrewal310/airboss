/*
  Helpers for writing hand-made fixtures compactly. Generated Rust fixtures won't need these:
  they arrive as a whole `MockSeed` (see index.ts).
*/

import type {
  Actor,
  Event,
  EventKind,
  Link,
  LinkKind,
  NeedsItem,
  PrState,
  PrView,
  Priority,
  Repo,
  RepoId,
  Status,
  Task,
  TaskId,
  ThreadState,
  Timestamp,
} from '../../types';
import type { MockSeed, MockThread } from '../model';

export const MIN = 60_000;
export const HOUR = 60 * MIN;
export const DAY = 24 * HOUR;

export interface TaskSpec {
  id: TaskId;
  title: string;
  status?: Status;
  parent?: TaskId | null;
  repos?: RepoId[];
  body?: string;
  blocked?: string;
  priority?: Priority;
  branch?: string;
  origin?: Task['origin'];
  /** How long ago it was created (ms). */
  created?: number;
}

/** Builds a seed around a fixed "now". */
export class SeedBuilder {
  readonly nowMs: number;
  repos: Repo[] = [];
  tasks: Task[] = [];
  edges: [TaskId, TaskId][] = [];
  links: Link[] = [];
  events: Event[] = [];
  threads: MockThread[] = [];
  prs: PrView[] = [];
  needs: NeedsItem[] = [];
  readySince: Record<number, Timestamp> = {};
  private positions = new Map<TaskId | null, number>();
  private eventId = 1;
  private linkId = 1;

  constructor(now: Timestamp) {
    this.nowMs = Date.parse(now);
  }

  at(agoMs: number): Timestamp {
    return new Date(this.nowMs - agoMs).toISOString();
  }

  repo(id: RepoId, name: string, remote?: string): this {
    this.repos.push({ id, name, path: `~/Code/${name}`, remote_url: remote ?? `git@github.com:acme/${name}.git` });
    return this;
  }

  task(spec: TaskSpec): this {
    const parent = spec.parent ?? null;
    const position = this.positions.get(parent) ?? 0;
    this.positions.set(parent, position + 1);
    const created = this.at(spec.created ?? 9 * DAY);
    const origin = spec.origin ?? (parent === null ? 'me' : 'plan');
    this.tasks.push({
      id: spec.id,
      parent,
      repos: spec.repos ?? [],
      title: spec.title,
      body: spec.body ?? null,
      status: spec.status ?? 'todo',
      blocked_reason: spec.blocked ?? null,
      position,
      origin,
      plan_key: null,
      branch: spec.branch ?? null,
      priority: spec.priority ?? null,
      created_at: created,
    });
    const by: Actor = origin === 'plan' ? 'plan' : origin === 'agent' ? 'agent:planner' : 'me';
    this.event(spec.id, 'created', spec.created ?? 9 * DAY, by);
    return this;
  }

  waits(task: TaskId, ...on: TaskId[]): this {
    for (const b of on) this.edges.push([task, b]);
    return this;
  }

  event(task: TaskId, kind: EventKind, agoMs: number, actor: Actor, extra: { to?: Status; body?: string } = {}): this {
    this.events.push({
      id: this.eventId++,
      task,
      kind,
      to_status: extra.to ?? null,
      body: extra.body ?? null,
      actor,
      at: this.at(agoMs),
    });
    return this;
  }

  /** A status change, recorded as an event. */
  moved(task: TaskId, to: Status, agoMs: number, actor: Actor, body?: string): this {
    return this.event(task, 'status', agoMs, actor, { to, ...(body ? { body } : {}) });
  }

  link(task: TaskId, kind: LinkKind, ref: string, opts: { title?: string; url?: string; pr?: PrState; ago?: number; by?: Actor } = {}): this {
    const ago = opts.ago ?? 2 * DAY;
    this.links.push({
      id: this.linkId++,
      task,
      kind,
      ref,
      url: opts.url ?? null,
      pr_state: opts.pr ?? null,
      title: opts.title ?? null,
      linked_by: opts.by ?? 'me',
      linked_at: this.at(ago),
    });
    // core writes "<kind> <ref>" (links.rs).
    this.event(task, 'linked', ago, opts.by ?? 'me', { body: `${kind} ${ref}` });
    return this;
  }

  thread(
    task: TaskId | TaskId[],
    ref: string,
    state: ThreadState,
    opts: { title?: string; repo?: string; error?: string; ask?: { id: string; kind: 'question' | 'approval'; text: string; ago: number }; ago?: number; source?: 't3' | 'claude' | 'herdr'; stale?: boolean; launchedAgo?: number; reply?: string } = {},
  ): this {
    const tasks = Array.isArray(task) ? task : [task];
    const source = opts.source ?? 't3';
    const worktree = source === 'herdr' ? `~/.herdr/worktrees/${opts.repo}/${opts.title ?? ref}` : `~/.t3/worktrees/${opts.repo}/${ref}`;
    this.threads.push({
      tasks,
      url: source === 't3' ? `t3code://threads/env-local/${ref}` : null,
      view: {
        key: { source, ref },
        linked: true,
        state,
        title: opts.title ?? ref,
        request: opts.ask ? { id: opts.ask.id, kind: opts.ask.kind, text: opts.ask.text, at: this.at(opts.ask.ago) } : null,
        error: opts.error ?? null,
        run_id: state === 'errored' ? `run-${ref}` : null,
        project_path: opts.repo ? `~/Code/${opts.repo}` : null,
        branch: null,
        worktree_path: opts.repo ? worktree : null,
        parent_ref: null,
        child_count: 0,
        last_activity_at: this.at(opts.ago ?? 10 * MIN),
        last_reply: opts.reply ?? null,
        stale: opts.stale ?? false,
      },
    });
    for (const t of tasks) {
      const by: Actor = opts.launchedAgo !== undefined ? 'launch' : 'me';
      const kind = ({ t3: 't3_thread', claude: 'claude_session', herdr: 'herdr_terminal' } as const)[source];
      const title = { t3: `T3 · ${opts.title ?? ref}`, claude: `Claude · ${opts.repo ?? ref}`, herdr: `Herdr · ${opts.title ?? ref}` }[source];
      this.link(t, kind, ref, {
        title,
        url: source === 't3' ? `t3code://threads/env-local/${ref}` : undefined,
        ago: opts.launchedAgo ?? 3 * DAY,
        by,
      });
    }
    return this;
  }

  pr(
    task: TaskId,
    ref: string,
    state: PrState,
    opts: { title?: string; add?: number; del?: number; checks?: PrView['checks']; review?: PrView['review']; ago?: number; head?: string; linked?: boolean } = {},
  ): this {
    const n = ref.split('#')[1];
    const [owner, repo] = ref.split('#')[0]!.split('/');
    const url = `https://github.com/${owner}/${repo}/pull/${n}`;
    this.prs.push({
      ref,
      linked: opts.linked ?? true,
      url,
      title: opts.title ?? null,
      state,
      draft: false,
      review: opts.review ?? 'none',
      checks: opts.checks ?? 'passing',
      additions: opts.add ?? null,
      deletions: opts.del ?? null,
      head_ref: opts.head ?? null,
      author: 'me',
      updated_at: this.at(opts.ago ?? HOUR),
      ready_since: this.at(opts.ago ?? HOUR),
      problem: null,
      stale: false,
    });
    if (opts.linked !== false) {
      this.link(task, 'pr', ref, { url, pr: state, title: `PR #${n}${opts.title ? ` · ${opts.title}` : ''}`, ago: opts.ago ?? HOUR, by: 'sync:gh' });
    }
    return this;
  }

  need(item: Omit<NeedsItem, 'group' | 'proposed' | 'stale' | 'task_title'> & { proposed?: [TaskId, string][]; stale?: boolean }): this {
    const group = GROUP[item.kind];
    const title = this.tasks.find((t) => t.id === item.task)?.title ?? '';
    this.needs.push({ ...item, group, task_title: title, proposed: item.proposed ?? [], stale: item.stale ?? false });
    return this;
  }

  build(rest: Omit<MockSeed, 'now' | 'repos' | 'tasks' | 'edges' | 'links' | 'events' | 'threads' | 'prs' | 'needs' | 'readySince'> & { now: Timestamp }): MockSeed {
    return {
      ...rest,
      repos: this.repos,
      tasks: this.tasks,
      edges: this.edges,
      links: this.links,
      events: this.events,
      threads: this.threads,
      prs: this.prs,
      needs: this.needs,
      readySince: this.readySince,
    };
  }
}

const GROUP: Record<NeedsItem['kind'], NeedsItem['group']> = {
  question: 'questions',
  approval: 'questions',
  review: 'reviews',
  failed_run: 'failed_runs',
  agent_stopped: 'stopped',
  proposed: 'proposed',
  ready: 'ready',
  quiet: 'quiet',
};
