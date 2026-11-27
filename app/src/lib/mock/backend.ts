/*
  The mock backend: answers every Appendix A command in the browser from a fixture, applies
  writes to its model, returns real patches (and sends each again as `tree:patch`), stands in
  for `attention` (needs follow ~250 ms after a write) and `launch` (Do it takes ~1.2 s), keeps
  an undo stack with the 5 s deferred reject, and throws `UnscriptedCall` for anything else.
*/

import type { ArgsOf, CommandName, EventName, Events, ResultOf } from '../api/contract';
import type { Transport } from '../api/transport';
import type {
  AppError,
  Availability,
  Connection,
  DoItPlace,
  DoItResult,
  EventKind,
  LaunchReport,
  LauncherKind,
  Mode,
  NeedsList,
  Patch,
  Repo,
  RepoId,
  RowDto,
  ServiceError,
  StartedThread,
  Status,
  TaskId,
  UndoEntry,
  UpdateStatus,
} from '../types';
import { classify } from '../logic/links';
import { codexNeedsTerminalId } from '../logic/connections';
import { recomputeNeeds, sortNeeds } from './attention';
import { isOpen, MOCK_VERSION, stateFromSeed, ts, type MockSeed, type MockState } from './model';
import {
  ancestors,
  buildDetail,
  buildPlans,
  buildRows,
  buildRunMap,
  diff,
  indexTree,
  openBlockers,
  parentOptions,
  prepare,
  rollUp,
  stepsOf,
  subtree,
  threadsFor,
  visibleNeeds,
} from './viewmodel';

/** A call the mock doesn't know. In tests this fails the test, as SPEC-app asks. */
export class UnscriptedCall extends Error {
  constructor(command: string) {
    super(`UnscriptedCall: the mock backend has no handler for "${command}"`);
    this.name = 'UnscriptedCall';
  }
}

export interface MockDelays {
  /** How long Do it and Plan it take. */
  launch: number;
  /** How long `attention` takes to follow a write. */
  attention: number;
  /** The deferred reject. */
  reject: number;
  /** A PR lookup after ⌘L, and a check for updates. */
  lookup: number;
  /** Update and restart, from the download's first step to Restarting… */
  install: number;
}

/** `errors.rs`'s MSG_HAS_STEPS. */
const HAS_STEPS = 'This task has subtasks: its status follows them, and PRs go on the subtasks.';

const DEFAULT_DELAYS: MockDelays = { launch: 1200, attention: 250, reject: 5000, lookup: 600, install: 1500 };

interface UndoRecord extends UndoEntry {
  tasks: TaskId[];
  inverse: () => void;
  /** A deferred reject: cancelled by undo, written when the timer fires. */
  timer?: ReturnType<typeof setTimeout>;
}

type Handler = (args: never) => unknown;

const LABEL: Record<Status, string> = {
  proposed: 'Proposed',
  todo: 'Todo',
  doing: 'In progress',
  in_review: 'In review',
  blocked: 'Blocked',
  done: 'Done',
  dropped: 'Dropped',
};

function fail(code: AppError['code'], message: string, detail: string | null = null): AppError {
  return { code, message, detail };
}

const T3_MISSING = 'Starting agents needs T3 Code. Install it, then choose Check again.';
const HERDR_MISSING = "Herdr isn't installed.";
const isReady = (a: Availability) => typeof a === 'object' && 'ready' in a;
const isMissing = (a: Availability) => typeof a === 'object' && 'missing' in a;
const BRANCH = /^[A-Za-z0-9._/-]+$/;

export class MockBackend implements Transport {
  readonly native = false;
  readonly s: MockState;
  /** Every call, in order (tests read it). */
  readonly calls: { command: string; args: unknown }[] = [];
  /** URLs and threads "opened" (tests read it). */
  readonly opened: string[] = [];
  private seq = 1;
  private last: { rows: Map<TaskId, RowDto>; order: TaskId[]; repos: Repo[] };
  private listeners = new Map<EventName, Set<(p: never) => void>>();
  private undoStack: UndoRecord[] = [];
  private nextUndo = 1;
  private launching = new Set<TaskId>();
  private needsVersion = { value: 1 };
  private attentionTimer: ReturnType<typeof setTimeout> | null = null;
  /** Update and restart is running: a second one does nothing. */
  private updating = false;
  private readonly t0 = Date.now();
  private readonly seedNow: number;
  private readonly delays: MockDelays;

  constructor(seed: MockSeed, opts: { delays?: Partial<MockDelays> } = {}) {
    this.s = stateFromSeed(seed);
    rollUp(this.s);
    this.s.needs = sortNeeds(this.s.needs);
    this.seedNow = this.s.nowMs;
    this.delays = { ...DEFAULT_DELAYS, ...opts.delays };
    const { rows, order } = buildRows(this.s);
    this.last = { rows, order, repos: structuredClone(this.s.repos) };
  }

  // ------------------------------------------------------------------ transport

  async invoke<K extends CommandName>(command: K, args: ArgsOf<K>): Promise<ResultOf<K>> {
    this.calls.push({ command, args });
    const handler = (this.handlers as Record<string, Handler | undefined>)[command];
    if (!handler) throw new UnscriptedCall(command);
    this.tick();
    const result = await handler(args as never);
    return structuredClone(result) as ResultOf<K>;
  }

  async listen<E extends EventName>(event: E, handler: (payload: Events[E]) => void): Promise<() => void> {
    const set = this.listeners.get(event) ?? new Set();
    set.add(handler as (p: never) => void);
    this.listeners.set(event, set);
    return () => set.delete(handler as (p: never) => void);
  }

  /** Sends an event as Rust would (asynchronously, as a copy). */
  emit<E extends EventName>(event: E, payload: Events[E]): void {
    const copy = structuredClone(payload);
    setTimeout(() => {
      for (const h of this.listeners.get(event) ?? []) (h as (p: Events[E]) => void)(copy);
    }, 0);
  }

  // ------------------------------------------------------------------ internals

  private tick(): void {
    this.s.nowMs = this.seedNow + (Date.now() - this.t0);
  }

  private now(): string {
    return ts(this.s.nowMs);
  }

  private task(id: TaskId) {
    const t = this.s.tasks.get(id);
    if (!t) throw fail('not_found', `Task #${id} no longer exists.`);
    return t;
  }

  private event(task: TaskId, kind: EventKind, extra: { to?: Status; body?: string; actor?: string } = {}) {
    this.s.events.push({
      id: this.s.nextEventId++,
      task,
      kind,
      to_status: extra.to ?? null,
      body: extra.body ?? null,
      actor: extra.actor ?? 'me',
      at: this.now(),
    });
  }

  /** Rebuild, diff and send `tree:patch`; then let `attention` follow. */
  private commit(created: TaskId[] = []): Patch {
    // buildRows works parents' statuses out from their steps first (SPEC-core rule 10).
    const { rows, order } = buildRows(this.s);
    const next = { rows, order, repos: structuredClone(this.s.repos) };
    const patch = diff(++this.seq, this.last, next, created);
    this.last = next;
    this.emit('tree:patch', patch);
    this.scheduleAttention();
    return patch;
  }

  private scheduleAttention(): void {
    if (this.attentionTimer) clearTimeout(this.attentionTimer);
    this.attentionTimer = setTimeout(() => {
      this.attentionTimer = null;
      this.tick();
      if (!recomputeNeeds(this.s, this.needsVersion)) return;
      this.emit('needs:changed', this.needsList());
      const { rows, order } = buildRows(this.s);
      const next = { rows, order, repos: structuredClone(this.s.repos) };
      const patch = diff(++this.seq, this.last, next);
      this.last = next;
      if (patch.upserts.length || patch.removed.length || patch.order) this.emit('tree:patch', patch);
    }, this.delays.attention);
  }

  needsList(): NeedsList {
    const items = sortNeeds(visibleNeeds(this.s));
    return { items, count: items.length, checked_at: ts(this.seedNow), version: this.needsVersion.value };
  }

  /** The newest agent event's time, as Rust's catch-up tracks it. */
  /** `connections:changed`, as Rust sends it. */
  private emitConnections(): void {
    this.emit('connections:changed', {
      connections: this.connectionRows(),
      availability: this.s.availability,
      herdr: this.herdr(),
      launcher: this.launcher(),
      agent_report: this.agentReport(),
      cli: this.s.cli,
      agents: this.s.agents,
    });
  }

  private showHerdr(ref: string): null {
    if (this.s.herdrFails) throw fail('herdr_failed', "Couldn't show that terminal in Herdr.");
    this.opened.push(`herdr:${ref}`);
    return null;
  }

  private herdr(): Availability {
    return this.s.herdr ?? { missing: { reason: HERDR_MISSING } };
  }

  /** Where Do it and Plan it start agents, as `Settings::launcher` picks. */
  private launcher(): LauncherKind {
    const pick = this.s.settings.start_agents_in;
    if (pick !== 'auto') return pick;
    return isMissing(this.s.availability) && isReady(this.herdr()) ? 'herdr' : 't3';
  }

  /** The rows as `connections.rs` merges them: Herdr's own row, and no T3 Code row for someone on Herdr alone. */
  private connectionRows(): Connection[] {
    const h = this.herdr();
    const pick = this.s.settings.start_agents_in;
    let rows = this.s.connections.filter((c) => c.name !== 'Herdr');
    if (typeof h === 'object' && 'ready' in h) {
      const detail =
        this.launcher() === 'herdr'
          ? `Herdr ${h.ready.version}. Agents you start on a task open here, each in its own tab.`
          : `Herdr ${h.ready.version}. To start agents here, choose it in Settings › Agent setup.`;
      rows = [{ name: 'Herdr', ok: true, state: 'Ready', detail, fix: null }, ...rows];
    } else if (typeof h === 'object' && 'missing' in h && pick === 'herdr') {
      rows = [
        { name: 'Herdr', ok: false, state: 'Not installed', detail: h.missing.reason, fix: 'Install Herdr, then choose Check again. Or start agents in T3 Code in Settings › Agent setup.' },
        ...rows,
      ];
    }
    if (isMissing(this.s.availability) && isReady(h) && pick !== 't3') rows = rows.filter((c) => c.name !== 'T3 Code');
    return rows;
  }

  /** The Agents row as Rust words it (`connections.rs`): the CLI, then an agent tool with the server. */
  private agentsRow(): Connection {
    if (!this.s.cli.installed) {
      return { name: 'Agents', ok: false, state: 'Not set up', detail: 'Agents report progress and ask questions through the airboss command.', fix: 'Install the command-line tool, then connect Claude Code.' };
    }
    if (!this.s.agents.claude && !this.s.agents.codex) {
      return { name: 'Agents', ok: false, state: 'Not connected', detail: "Agents can't report to Airboss yet.", fix: "Agents can't report to Airboss yet. Connect Claude Code in Settings › Agent setup." };
    }
    if (codexNeedsTerminalId(this.s.agents, this.launcher(), this.s.settings.herdr_agent)) {
      return {
        name: 'Agents',
        ok: false,
        state: 'Out of date',
        detail: 'Codex setup needs HC_TERMINAL_ID',
        fix: 'Codex setup needs HC_TERMINAL_ID. Copy the Codex setup and put it in place of [mcp_servers.airboss] in ~/.codex/config.toml.',
      };
    }
    return { name: 'Agents', ok: true, state: 'Ready', detail: 'Agents report progress and ask questions through the airboss command.', fix: null };
  }

  private setAgentsRow(): void {
    const row = this.agentsRow();
    this.s.connections = this.s.connections.some((c) => c.name === 'Agents')
      ? this.s.connections.map((c) => (c.name === 'Agents' ? row : c))
      : [...this.s.connections, row];
  }

  private agentReport(): string | null {
    let at: string | null = null;
    for (const e of this.s.events) if (e.actor.startsWith('agent:') && (at === null || e.at > at)) at = e.at;
    return at;
  }

  private pushUndo(label: string, tasks: TaskId[], inverse: () => void): UndoRecord {
    const rec: UndoRecord = { id: this.nextUndo++, label, tasks, inverse };
    this.undoStack.push(rec);
    if (this.undoStack.length > 20) this.undoStack.shift();
    return rec;
  }

  private entry(rec: UndoRecord): UndoEntry {
    return { id: rec.id, label: rec.label };
  }

  private setStatusRaw(id: TaskId, status: Status, note: string | null, actor = 'me') {
    const t = this.task(id);
    if (status === 'done' && t.status !== 'done') this.s.statusBefore.set(id, t.status);
    t.status = status;
    t.blocked_reason = status === 'blocked' ? note : null;
    this.event(id, 'status', { to: status, actor, ...(note ? { body: note } : {}) });
  }

  /** SPEC-core rule 10: a task with steps takes no status change but Drop. */
  private hasSteps(id: TaskId): boolean {
    return stepsOf(this.s, indexTree(this.s), id).length > 0;
  }

  /** Rule 12: under a task with steps, or an open task that isn't a proposal. */
  private ensureOpenParent(id: TaskId): void {
    const p = this.task(id);
    if (this.hasSteps(id)) return;
    if (!isOpen(p.status) || p.status === 'proposed') {
      throw fail('invalid_target', "Tasks can't go under a proposed, done or dropped task.");
    }
  }

  private repoName(id: RepoId): string {
    return this.s.repos.find((r) => r.id === id)?.name ?? `repo ${id}`;
  }

  // ------------------------------------------------------------------ Do it

  private async launch(id: TaskId, mode: Mode, place: DoItPlace | null, again: boolean): Promise<DoItResult> {
    const t = this.task(id);
    const launcher = this.launcher();
    const herdr = launcher === 'herdr';
    if (this.launching.has(id)) throw fail('already_starting', `Already starting — check ${herdr ? 'Herdr' : 'T3'} in a moment.`);
    if (!herdr && isMissing(this.s.availability)) throw fail('t3_missing', T3_MISSING);
    if (herdr && isMissing(this.herdr())) throw fail('herdr_missing', HERDR_MISSING);
    if (t.status === 'done' || t.status === 'dropped') throw fail('task_closed', `${id} is ${t.status === 'done' ? 'done' : 'dropped'}.`);
    if (t.status === 'proposed') throw fail('task_proposed', `${id} is still a proposal. Accept it first.`);
    const ix = indexTree(this.s);
    if (mode === 'do_it') {
      const waiting = openBlockers(this.s, ix, id);
      if (waiting.length) throw fail('waiting', `Task ${id} waits on ${waiting.join(' and ')}.`);
    }
    if (place && typeof place === 'object' && 'repo' in place) {
      if (!t.repos.includes(place.repo)) {
        t.repos.push(place.repo);
        this.event(id, 'repo_linked', { body: this.repoName(place.repo) });
      }
    }
    const prepared = prepare(this.s, ix, id)!;
    const repos = place === 'no_repo' ? [] : prepared.repos.map((r) => r.id);
    if (mode === 'do_it' && repos.length === 0 && place !== 'no_repo') {
      return { needs_choice: { choices: prepared.choices, research_dir: prepared.research_dir } };
    }
    this.launching.add(id);
    try {
      if (this.delays.launch) await new Promise((r) => setTimeout(r, this.delays.launch));
    } finally {
      this.launching.delete(id);
    }
    this.tick();
    const report: LaunchReport = { started: [], skipped: [], failed: [], branch: prepared.branch, moved_to_doing: false };
    const targets: (RepoId | null)[] = mode === 'plan_it' ? [repos[0] ?? null] : repos.length ? repos : [null];
    const existing = threadsFor(this.s, id).filter((th) => th.view.state === 'running' || th.view.state === 'needs_me');
    for (const repo of targets) {
      const repoName = repo === null ? null : this.repoName(repo);
      if (!again && mode === 'do_it' && existing.some((th) => th.view.project_path === `~/Code/${repoName}`)) {
        if (repo !== null) report.skipped.push(repo);
        continue;
      }
      if (!herdr && repo !== null && this.s.notProjects.includes(repo)) {
        const err: ServiceError = { code: 'not_a_project', message: `${repoName} isn't a T3 project yet.` };
        report.failed.push([repo, err]);
        continue;
      }
      // A Herdr terminal's address counts up (`t4p`); a T3 thread has its own id.
      const ref = herdr ? `t${this.s.nextLinkId}p` : `${mode === 'plan_it' ? 'plan' : 'task'}-${id}${repoName ? `-${repoName}` : ''}-${this.s.nextLinkId}`;
      const title = mode === 'plan_it' ? `Plan ${id}` : repoName ? `${id} in ${repoName}` : `${id} research`;
      const source = herdr ? 'herdr' : 't3';
      const url = herdr ? null : `t3code://threads/env-local/${ref}`;
      const kind = herdr ? 'herdr_terminal' : 't3_thread';
      this.s.threads.set(`${source}:${ref}`, {
        tasks: [id],
        url,
        view: {
          key: { source, ref },
          linked: true,
          state: 'running',
          title,
          request: null,
          error: null,
          run_id: null,
          project_path: repoName ? `~/Code/${repoName}` : prepared.research_dir,
          branch: prepared.branch,
          worktree_path: null,
          parent_ref: null,
          child_count: 0,
          last_activity_at: this.now(),
          last_reply: null,
          stale: false,
        },
      });
      this.s.links.push({
        id: this.s.nextLinkId++,
        task: id,
        kind,
        ref,
        url,
        pr_state: null,
        title: `${herdr ? 'Herdr' : 'T3'} · ${title}`,
        linked_by: 'launch',
        linked_at: this.now(),
      });
      this.event(id, 'linked', { actor: 'launch', body: `${kind} ${ref}` });
      const started: StartedThread = { key: { source, ref }, mode, repo, title, linked: true };
      report.started.push(started);
    }
    // A parent takes no branch and no status of its own (SPEC-core rule 10).
    const parent = this.hasSteps(id);
    if (report.started.length && !parent && !t.branch && prepared.branch) t.branch = prepared.branch;
    if (report.started.length && !parent && mode === 'do_it' && (t.status === 'todo' || t.status === 'blocked')) {
      this.setStatusRaw(id, 'doing', null, 'launch');
      report.moved_to_doing = true;
    }
    return { started: { report, patch: this.commit() } };
  }

  // ------------------------------------------------------------------ handlers

  private readonly handlers: { [K in CommandName]: (args: ArgsOf<K>) => ResultOf<K> | Promise<ResultOf<K>> } = {
    bootstrap: () => {
      if (this.s.dbError) throw fail('db_unavailable', this.s.dbError.reason, this.s.dbError.path);
      const { rows, order } = this.last;
      return {
        seq: this.seq,
        rows: order.map((id) => rows.get(id)!),
        order,
        repos: this.s.repos,
        needs: this.needsList(),
        connections: this.connectionRows(),
        availability: this.s.availability,
        herdr: this.herdr(),
        launcher: this.launcher(),
        agent_report: this.agentReport(),
        settings: this.s.settings,
        accent: null,
        cli: this.s.cli,
        agents: this.s.agents,
      };
    },

    task_detail: ({ id, req, historyLimit }) => {
      this.task(id);
      return buildDetail(this.s, id, req, historyLimit);
    },

    run_map: ({ id }) => {
      this.task(id);
      return buildRunMap(this.s, indexTree(this.s), id);
    },

    plans: () => buildPlans(this.s),

    quickadd_data: () => ({ repos: this.s.repos, parents: parentOptions(this.s) }),

    t3_projects: () =>
      this.s.t3Projects.map((p) => ({
        path: p.path,
        name: p.name,
        is_git: this.s.gitFolders.includes(p.path),
        added: this.s.repos.some((r) => r.path === p.path),
      })),

    herdr_folders: () => {
      if (!isReady(this.herdr())) return [];
      return (this.s.herdrFolders ?? []).map((f) => ({
          path: f.folder,
          name: f.name,
          is_git: this.s.gitFolders.includes(f.folder),
          added: this.s.repos.some((r) => r.path === f.folder),
        }));
    },

    create_task: async ({ task: input }) => {
      const title = input.title.trim();
      if (!title) throw fail('empty_title', 'A task needs a title.');
      if (input.parent !== null) this.ensureOpenParent(input.parent);
      const id = Math.max(0, ...this.s.tasks.keys()) + 1;
      const siblings = [...this.s.tasks.values()].filter((t) => t.parent === input.parent);
      this.s.tasks.set(id, {
        id,
        parent: input.parent,
        repos: [...input.repos],
        title,
        body: input.body?.trim() || null,
        status: 'todo',
        blocked_reason: null,
        position: siblings.length ? Math.max(...siblings.map((t) => t.position)) + 1 : 0,
        origin: 'me',
        plan_key: null,
        branch: null,
        priority: null,
        created_at: this.now(),
      });
      this.event(id, 'created');
      const link_errors: { text: string; error: AppError }[] = [];
      for (const l of input.links) {
        const c = classify(l.text);
        if (!c) {
          link_errors.push({ text: l.text, error: fail('invalid_target', "That isn't a link or a path Airboss understands.") });
          continue;
        }
        this.addLink(id, c, l.title);
      }
      const patch = this.commit([id]);
      let launch: DoItResult | null = null;
      if (input.then !== 'none') {
        try {
          launch = await this.launch(id, input.then, null, false);
        } catch (e) {
          const err = e as AppError;
          this.emit('toast', { text: err.message, actions: [], undo: null });
          this.emit('nav:open', { view: 'all', task: id, needs_key: null });
        }
      }
      return { id, patch, launch, link_errors };
    },

    rename_task: ({ id, title }) => {
      const t = this.task(id);
      if (!title.trim()) throw fail('empty_title', 'A task needs a title.');
      t.title = title.trim();
      this.event(id, 'edited', { body: 'title' });
      return this.commit();
    },

    edit_notes: ({ id, body }) => {
      const t = this.task(id);
      t.body = body?.trim() ? body : null;
      this.event(id, 'edited', { body: 'body' });
      return this.commit();
    },

    set_status: ({ id, status, note }) => {
      const t = this.task(id);
      if (status === 'proposed') throw fail('not_allowed', "That change isn't allowed.");
      if (status === 'blocked' && !note?.trim()) throw fail('invalid_transition', 'A blocked task needs a reason.');
      const parent = this.hasSteps(id);
      if (parent && status !== 'dropped') throw fail('invalid_target', HAS_STEPS);
      const before = new Map<TaskId, { status: Status; reason: string | null }>();
      // As in Rust: a parent changes only through its steps, so a drop touches its open steps.
      const ix = indexTree(this.s);
      const touched = status === 'dropped' ? [id, ...subtree(ix, id).filter((k) => isOpen(this.s.tasks.get(k)!.status))] : [id];
      for (const k of [...touched]) if (stepsOf(this.s, ix, k).length) touched.splice(touched.indexOf(k), 1);
      for (const k of touched) {
        const x = this.s.tasks.get(k)!;
        before.set(k, { status: x.status, reason: x.blocked_reason });
      }
      if ((t.status === status && status !== 'blocked') || !touched.length) return { patch: null, undo: null };
      for (const k of touched) this.setStatusRaw(k, status, k === id ? note : null);
      const label = status === 'done' ? `Mark done #${id}` : status === 'dropped' ? `Drop #${id}` : status === 'blocked' ? `Mark blocked #${id}` : `Move #${id} to ${LABEL[status]}`;
      // As in Rust: a dropped proposal stays dropped (me can't make a proposal again).
      const rec = this.pushUndo(label, touched, () => {
        for (const [k, b] of before) if (!(status === 'dropped' && b.status === 'proposed')) this.setStatusRaw(k, b.status, b.reason);
      });
      return { patch: this.commit(), undo: this.entry(rec) };
    },

    toggle_done: ({ id }) => {
      const t = this.task(id);
      if (this.hasSteps(id)) throw fail('invalid_target', HAS_STEPS);
      const prev = { status: t.status, reason: t.blocked_reason };
      const to: Status = t.status === 'done' ? (this.s.statusBefore.get(id) ?? 'todo') : 'done';
      if (to === 'blocked') this.setStatusRaw(id, 'blocked', prev.reason ?? 'Blocked');
      else this.setStatusRaw(id, to, null);
      const rec = this.pushUndo(to === 'done' ? `Mark done #${id}` : `Mark not done #${id}`, [id], () =>
        this.setStatusRaw(id, prev.status, prev.reason),
      );
      return { patch: this.commit(), undo: this.entry(rec) };
    },

    accept_proposals: ({ ids }) => {
      for (const id of ids) {
        if (this.task(id).status !== 'proposed') continue;
        this.setStatusRaw(id, 'todo', null);
      }
      return this.commit();
    },

    reject_proposals: ({ ids }) => {
      for (const id of ids) this.task(id);
      const label = ids.length === 1 ? `Reject #${ids[0]}` : `Reject ${ids.length} subtasks`;
      const rec = this.pushUndo(label, ids, () => {
        if (rec.timer) clearTimeout(rec.timer);
      });
      rec.timer = setTimeout(() => {
        this.tick();
        this.undoStack = this.undoStack.filter((r) => r !== rec);
        for (const id of ids) {
          const t = this.s.tasks.get(id);
          if (!t || t.status !== 'proposed') continue;
          t.status = 'dropped';
          this.event(id, 'rejected');
        }
        this.commit();
      }, this.delays.reject);
      return { patch: null, undo: this.entry(rec) };
    },

    move_task: ({ id, parent, position }) => {
      const t = this.task(id);
      if (parent !== null) {
        this.task(parent);
        if (parent === id || ancestors(this.s, parent).includes(id)) {
          throw fail('invalid_target', "A task can't go under itself or one of its subtasks.");
        }
        this.ensureOpenParent(parent);
      }
      const prev = { parent: t.parent, position: t.position };
      this.place(id, parent, position);
      this.event(id, 'moved');
      const rec = this.pushUndo(`Move #${id}`, [id], () => {
        this.place(id, prev.parent, prev.position);
        this.event(id, 'moved');
      });
      return { patch: this.commit(), undo: this.entry(rec) };
    },

    reorder: ({ id, direction }) => {
      const t = this.task(id);
      // As in Rust: past hidden siblings (dropped ones), to the visible neighbour.
      const siblings = [...this.s.tasks.values()]
        .filter((x) => x.parent === t.parent && (x.status !== 'dropped' || x.id === id))
        .sort((a, b) => a.position - b.position);
      const i = siblings.findIndex((x) => x.id === id);
      const j = direction === 'up' ? i - 1 : i + 1;
      const other = siblings[j];
      if (!other) return { patch: null, undo: null };
      const swap = () => {
        const a = this.s.tasks.get(id)!;
        const b = this.s.tasks.get(other.id)!;
        [a.position, b.position] = [b.position, a.position];
      };
      swap();
      this.event(id, 'moved');
      const rec = this.pushUndo(`Move #${id} ${direction}`, [id, other.id], swap);
      return { patch: this.commit(), undo: this.entry(rec) };
    },

    link_repo: ({ id, repo }) => {
      const t = this.task(id);
      if (!t.repos.includes(repo)) {
        t.repos.push(repo);
        this.event(id, 'repo_linked', { body: this.repoName(repo) });
      }
      return this.commit();
    },

    unlink_repo: ({ id, repo }) => {
      const t = this.task(id);
      t.repos = t.repos.filter((r) => r !== repo);
      this.event(id, 'repo_unlinked', { body: this.repoName(repo) });
      return this.commit();
    },

    add_repos: ({ paths }) => {
      const added: Repo[] = [];
      for (const path of paths) {
        const name = path.replace(/\/+$/, '').split('/').pop() ?? path;
        if (this.s.repos.some((r) => r.path === path || r.name === name)) {
          throw fail('already_exists', 'A repo with that name or folder is already added.');
        }
        const repo: Repo = { id: Math.max(0, ...this.s.repos.map((r) => r.id)) + 1, name, path, remote_url: null };
        this.s.repos.push(repo);
        added.push(repo);
      }
      return { repos: added, patch: this.commit() };
    },

    edit_repo: ({ repo, name, path, remote }) => {
      const r = this.s.repos.find((x) => x.id === repo);
      if (!r) throw fail('not_found', 'That repo no longer exists.');
      if (!name.trim()) throw fail('invalid_target', 'A repo needs a name.');
      if (this.s.repos.some((x) => x.id !== repo && (x.name === name.trim() || x.path === path))) {
        throw fail('already_exists', 'A repo with that name or folder is already added.');
      }
      Object.assign(r, { name: name.trim(), path, remote_url: remote?.trim() || null });
      return this.commit();
    },

    remove_repo: ({ repo }) => {
      if (!this.s.repos.some((x) => x.id === repo)) throw fail('not_found', 'That repo no longer exists.');
      const unlinked: TaskId[] = [];
      for (const t of this.s.tasks.values()) {
        if (!t.repos.includes(repo)) continue;
        t.repos = t.repos.filter((x) => x !== repo);
        unlinked.push(t.id);
      }
      this.s.repos = this.s.repos.filter((x) => x.id !== repo);
      return { unlinked: unlinked.sort((a, b) => a - b), patch: this.commit() };
    },

    back_up_database: () => '/Users/me/Documents/Airboss backup.db',

    pick_folder: () => '~/Code/side-project',

    set_branch: ({ id, branch }) => {
      const t = this.task(id);
      const b = branch.trim();
      if (b && !BRANCH.test(b)) {
        throw fail('invalid_target', 'Branch names can use letters, digits, `.`, `_`, `/` and `-`.');
      }
      t.branch = b || null;
      this.event(id, 'edited', { body: b ? `branch: ${b}` : 'branch cleared' });
      return this.commit();
    },

    add_dependency: ({ id, waitsOn }) => {
      this.task(id);
      this.task(waitsOn);
      const ix = indexTree(this.s);
      if (id === waitsOn || ancestors(this.s, id).includes(waitsOn) || subtree(ix, id).includes(waitsOn)) {
        throw fail('invalid_target', "A task can't wait on its own parent or subtask.");
      }
      // A loop: waitsOn already waits (directly or not) on id.
      const seen = new Set<TaskId>();
      const stack = [waitsOn];
      while (stack.length) {
        const n = stack.pop()!;
        for (const [task, on] of this.s.edges) {
          if (task !== n || seen.has(on)) continue;
          if (on === id) throw fail('loop', `That would make a loop: #${waitsOn} already waits on #${id}.`);
          seen.add(on);
          stack.push(on);
        }
      }
      if (!this.s.edges.some(([a, b]) => a === id && b === waitsOn)) {
        this.s.edges.push([id, waitsOn]);
        this.event(id, 'waits_on_added', { body: `waits on #${waitsOn}` });
      }
      return this.commit();
    },

    remove_dependency: ({ id, waitsOn }) => {
      this.task(id);
      this.s.edges = this.s.edges.filter(([a, b]) => !(a === id && b === waitsOn));
      this.event(id, 'waits_on_removed', { body: `no longer waits on #${waitsOn}` });
      return this.commit();
    },

    add_link: ({ id, text, title }) => {
      this.task(id);
      const c = classify(text);
      if (!c) throw fail('invalid_target', "That isn't a link or a path Airboss understands.");
      this.addLink(id, c, title);
      return this.commit();
    },

    remove_link: ({ link }) => {
      const l = this.s.links.find((x) => x.id === link);
      if (!l) throw fail('not_found', 'That link no longer exists.');
      this.s.links = this.s.links.filter((x) => x.id !== link);
      this.event(l.task, 'unlinked', { body: `${l.kind} ${l.ref}` });
      return this.commit();
    },

    add_note: ({ id, text }) => {
      this.task(id);
      if (!text.trim()) throw fail('empty_title', 'A note needs some text.');
      this.event(id, 'note', { body: text.trim() });
      return this.commit();
    },

    do_it: ({ id, place, again }) => this.launch(id, 'do_it', place, again),

    plan_it: ({ id, again }) => this.launch(id, 'plan_it', null, again),

    add_to_t3: ({ repo }) => {
      this.s.notProjects = this.s.notProjects.filter((r) => r !== repo);
      return null;
    },

    reopen_and_start: async ({ id }) => {
      this.setStatusRaw(id, 'todo', null);
      this.commit();
      return this.launch(id, 'do_it', null, false);
    },

    open_thread: ({ key }) => {
      // Rust shows a Herdr terminal by its address (herdr-app do agent.focus), known to sync or not.
      if (key.source === 'herdr') return this.showHerdr(key.ref);
      const th = this.s.threads.get(`${key.source}:${key.ref}`);
      if (!th) throw fail('not_found', 'That thread no longer exists.');
      this.opened.push(th.url ?? th.view.project_path ?? key.ref);
      return null;
    },

    open_url: ({ target }) => {
      if ('url' in target) {
        // Rust's opener.rs is the real allowlist; the mock keeps its outline.
        if (!/^(https:\/\/|file:\/\/\/|t3code:\/\/threads\/)/i.test(target.url))
          throw fail('url_not_allowed', 'Airboss only opens https links, files and T3 threads.');
        this.opened.push(target.url);
      } else {
        const l = this.s.links.find((x) => x.id === target.link);
        if (!l) throw fail('not_found', 'That link no longer exists.');
        if (l.kind === 'herdr_terminal') return this.showHerdr(l.ref);
        this.opened.push(l.url ?? l.ref);
      }
      return null;
    },

    needs_dismiss: ({ key }) => this.hideNeed(key, 'Dismiss'),

    needs_snooze: ({ key }) => this.hideNeed(key, 'Snooze'),

    needs_restore: ({ key }) => {
      this.s.hiddenNeeds.delete(key);
      this.needsVersion.value++;
      this.emit('needs:changed', this.needsList());
      return null;
    },

    undo: ({ entry }) => {
      const rec = entry === null ? this.undoStack[this.undoStack.length - 1] : this.undoStack.find((r) => r.id === entry);
      if (!rec) throw fail('not_found', 'Nothing to undo.');
      this.undoStack = this.undoStack.filter((r) => r !== rec);
      for (const id of rec.tasks) {
        if (!this.s.tasks.has(id)) throw fail('undo_refused', `${id} changed since; nothing was undone.`);
      }
      rec.inverse();
      const patch = rec.timer !== undefined ? null : this.commit();
      return { patch, message: `Undid ${rec.label.charAt(0).toLowerCase()}${rec.label.slice(1)}` };
    },

    recheck_connections: () => {
      setTimeout(() => this.emitConnections(), 400);
      return null;
    },

    copy_text: ({ what }) => {
      const text =
        typeof what === 'object'
          ? 'link' in what
            ? what.link
            : String(what.task_number)
          : what === 'agent_note'
            ? 'Airboss tracks this work. Use the airboss MCP tools to report progress.'
            : what === 'codex_setup'
              ? '[mcp_servers.airboss]\ncommand = "/Users/you/.local/bin/airboss"\nargs = ["mcp"]\nenv_vars = ["T3CODE_THREAD_ID", "HC_TERMINAL_ID"]\n'
              : 'claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp';
      this.opened.push(`clipboard:${text}`);
      void globalThis.navigator?.clipboard?.writeText(text).catch(() => {});
      return null;
    },

    install_cli: () => {
      this.s.cli = { ...this.s.cli, installed: true };
      this.setAgentsRow();
      this.emitConnections();
      return { path: this.s.cli.path, on_path: this.s.cli.on_path };
    },

    connect_claude: async () => {
      if (!this.s.cli.installed) throw fail('not_found', 'Install the command-line tool first, then connect Claude Code.');
      await new Promise((r) => setTimeout(r, this.delays.lookup));
      if (this.s.connectClaudeError) throw fail('internal', this.s.connectClaudeError);
      this.opened.push('claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp');
      this.s.agents = { ...this.s.agents, claude: true };
      this.setAgentsRow();
      this.emitConnections();
      return null;
    },

    get_settings: () => this.s.settings,

    set_settings: ({ settings }) => {
      if (settings.hotkey === 'Command+Shift+Space') {
        throw fail('not_allowed', 'That key is in use by another app. Pick another.');
      }
      const before = this.launcher();
      const pickedBefore = this.s.settings.start_agents_in;
      const agentBefore = this.s.settings.herdr_agent;
      this.s.settings = structuredClone(settings);
      if (this.launcher() !== before || settings.start_agents_in !== pickedBefore || settings.herdr_agent !== agentBefore) {
        this.setAgentsRow();
        this.emitConnections();
      }
      return this.s.settings;
    },

    notification_permission: () => this.s.notificationPermission ?? 'granted',

    open_notification_settings: () => {
      this.opened.push('system-settings:notifications');
      return null;
    },

    set_folded: ({ ids, folded }) => {
      const set = new Set(this.s.settings.folded);
      for (const id of ids) {
        if (folded) set.add(id);
        else set.delete(id);
      }
      this.s.settings.folded = [...set];
      return null;
    },

    set_focus_context: () => null,
    ui_ready: () => null,
    quickadd_shown: () => null,
    quickadd_hide: () => {
      this.emit('quickadd:reset', { hotkey: this.s.settings.hotkey, keep_text: false });
      return null;
    },
    quickadd_resize: () => null,

    show_main: ({ view, task, needsKey }) => {
      this.emit('nav:open', { view, task, needs_key: needsKey });
      return null;
    },

    // In-app updates (SPEC-updates), as commands/updates.rs answers them.
    update_status: () => this.s.updateStatus,

    check_for_updates: async () => {
      const u = this.s.update ?? { current: MOCK_VERSION, latest: null };
      if (this.delays.lookup) await new Promise((r) => setTimeout(r, this.delays.lookup));
      // A check while an install runs changes nothing.
      if (this.updating) return this.s.updateStatus;
      if (u.unavailable) {
        this.emit('toast', { text: 'Updates work only in the installed Airboss app.', actions: [], undo: null });
      } else if (u.checkError) {
        this.emit('toast', { text: `Couldn't check for updates: ${u.checkError}`, actions: [], undo: null, tone: 'warning' });
      } else if (!u.latest) {
        this.setUpdate({ state: 'idle' });
        this.emit('toast', { text: `You have the newest Airboss, ${u.current}.`, actions: [], undo: null });
      } else {
        this.setUpdate({ state: 'available', ...u.latest });
        this.emit('toast', {
          text: `Airboss ${u.latest.version} is available.`,
          actions: [{ label: 'Update and restart', command: 'install_update', args: {} }],
          undo: null,
        });
      }
      return this.s.updateStatus;
    },

    install_update: () => {
      if (this.updating) return null;
      const s = this.s.updateStatus;
      if (s.state !== 'available' && s.state !== 'failed') throw fail('not_found', "There's no update to install.");
      this.updating = true;
      const version = s.version;
      const error = this.s.update?.installError;
      const steps: UpdateStatus[] = [0, 25, 50, 75, 100].map((percent) => ({ state: 'downloading', version, percent }));
      steps.push(error ? { state: 'failed', version, message: error } : { state: 'installing', version });
      if (!error) steps.push({ state: 'restarting', version });
      steps.forEach((step, i) =>
        setTimeout(() => {
          this.setUpdate(step);
          if (step.state === 'failed') this.updating = false;
        }, (i * this.delays.install) / (steps.length - 1)),
      );
      return null;
    },
  };

  /** The update's new status, sent as `update:changed` when it changed. */
  private setUpdate(next: UpdateStatus): void {
    if (JSON.stringify(next) === JSON.stringify(this.s.updateStatus)) return;
    this.s.updateStatus = next;
    this.emit('update:changed', next);
  }

  private place(id: TaskId, parent: TaskId | null, position: number | null) {
    const t = this.s.tasks.get(id)!;
    const siblings = [...this.s.tasks.values()]
      .filter((x) => x.parent === parent && x.id !== id)
      .sort((a, b) => a.position - b.position);
    const at = position === null ? siblings.length : Math.max(0, Math.min(position, siblings.length));
    siblings.splice(at, 0, t);
    t.parent = parent;
    siblings.forEach((x, i) => (x.position = i));
  }

  private addLink(id: TaskId, c: NonNullable<ReturnType<typeof classify>>, title: string | null) {
    const link = {
      id: this.s.nextLinkId++,
      task: id,
      kind: c.kind,
      ref: c.ref,
      url: c.url,
      pr_state: null,
      title,
      linked_by: 'me',
      linked_at: this.now(),
    };
    this.s.links.push(link);
    this.event(id, 'linked', { body: `${c.kind} ${c.ref}` });
    if (c.kind === 'pr' && !this.s.prs.has(c.ref)) {
      // sync.lookup_pr answers in the background; the title and state fill in.
      setTimeout(() => {
        this.tick();
        const n = c.ref.split('#')[1];
        this.s.prs.set(c.ref, {
          ref: c.ref,
          linked: true,
          url: c.url,
          title: 'Fix double booking',
          state: 'open',
          draft: false,
          review: 'required',
          checks: 'pending',
          additions: 42,
          deletions: 7,
          head_ref: null,
          author: 'me',
          updated_at: this.now(),
          ready_since: this.now(),
          problem: null,
          stale: false,
        });
        const l = this.s.links.find((x) => x.id === link.id);
        if (!l) return;
        l.pr_state = 'open';
        l.title = title ?? `PR #${n} · Fix double booking`;
        this.event(id, 'link_state', { actor: 'sync:gh', body: `${c.ref} open` });
        this.commit();
      }, this.delays.lookup);
    }
  }

  private hideNeed(key: string, label: string) {
    if (!this.s.needs.some((n) => n.key === key)) throw fail('not_found', 'That item is no longer there.');
    this.s.hiddenNeeds.add(key);
    const rec = this.pushUndo(label, [], () => {
      this.s.hiddenNeeds.delete(key);
      this.needsVersion.value++;
      this.emit('needs:changed', this.needsList());
    });
    setTimeout(() => {
      this.needsVersion.value++;
      this.emit('needs:changed', this.needsList());
      this.commit();
    }, this.delays.attention);
    return { patch: null, undo: this.entry(rec) };
  }
}

export function asAppError(e: unknown): AppError {
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) return e as AppError;
  return { code: 'internal', message: 'Something went wrong inside Airboss; details are in the log.', detail: String(e) };
}
