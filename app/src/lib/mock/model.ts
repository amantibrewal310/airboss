/*
  The mock backend's in-memory model: what `core`, `sync`, `launch` and `attention` would hold.
  The mock builds rows, details, run maps and plan cards from it the way SPEC-app's view-model
  table describes, so writes return real patches.
*/

import type {
  Actor,
  AgentTools,
  Availability,
  CliStatus,
  Connection,
  Event,
  Link,
  NeedsItem,
  NotificationPermission,
  PrView,
  Repo,
  RepoId,
  Settings,
  Status,
  T3Project,
  Task,
  TaskId,
  ThreadView,
  Timestamp,
  UpdateStatus,
} from '../types';

/** A linked thread, with the tasks it's linked to and the URL `sync.thread_url` would give. */
export interface MockThread {
  view: ThreadView;
  tasks: TaskId[];
  url: string | null;
}

/** In-app updates (SPEC-updates): what this Airboss is, and what a check finds. */
export interface MockUpdate {
  /** The running version, for "You have the newest Airboss, …". */
  current: string;
  /** The newer release a check finds, or none. */
  latest: { version: string; notes_url: string } | null;
  /** The status at start (an automatic check already found `latest`); idle if unset. */
  status?: UpdateStatus;
  /** A check fails with this reason (offline, a bad manifest). */
  checkError?: string;
  /** Not an installed release build: updates work only in the installed app. */
  unavailable?: boolean;
  /** The install fails with this message once the download is done. */
  installError?: string;
}

/** The version the mock says it is, when a fixture doesn't say. */
export const MOCK_VERSION = '0.1.0-alpha.4';

/** Everything a fixture provides. Generated Rust fixtures will produce the same shape. */
export interface MockSeed {
  name: string;
  /** The fixture's "now", so times and screenshots are stable. */
  now: Timestamp;
  repos: Repo[];
  tasks: Task[];
  /** (task, waits_on), as `CoreSnapshot.edges`. */
  edges: [TaskId, TaskId][];
  links: Link[];
  events: Event[];
  threads: MockThread[];
  prs: PrView[];
  needs: NeedsItem[];
  /** When each ready task became ready (`ReadyTask.since`). */
  readySince: Record<number, Timestamp>;
  connections: Connection[];
  availability: Availability;
  /** Whether Herdr is there to start agents in (missing if unset). */
  herdr?: Availability;
  /** `herdr-app ls`'s space folders, then its recent ones (for `herdr_folders`). */
  herdrFolders?: { name: string; folder: string }[];
  /** Open thread on a Herdr terminal fails, as when Herdr can't show it. */
  herdrFails?: boolean;
  settings: Settings;
  cli: CliStatus;
  /** Which agent tools have the airboss MCP server (Rust reads their config files). */
  agents: AgentTools;
  /** `connect_claude` fails with this message, as when `claude` isn't found. */
  connectClaudeError?: string;
  t3Projects: T3Project[];
  /** Repo folders that exist and are git repositories (for `t3_projects`). */
  gitFolders: string[];
  /** Repos T3 doesn't know as projects: Do it fails there with NotAProject until "Add to T3". */
  notProjects: RepoId[];
  /** `bootstrap` fails as if the database won't open. */
  dbError?: { path: string; reason: string };
  /** What macOS says about notifications (granted if unset). */
  notificationPermission?: NotificationPermission;
  /** In-app updates: nothing newer if unset. */
  update?: MockUpdate;
}

export interface MockState extends Omit<MockSeed, 'tasks' | 'threads' | 'prs'> {
  nowMs: number;
  tasks: Map<TaskId, Task>;
  threads: Map<string, MockThread>;
  prs: Map<string, PrView>;
  hiddenNeeds: Set<string>;
  /** The status a done task had before, for toggling it back. */
  statusBefore: Map<TaskId, Status>;
  nextEventId: number;
  nextLinkId: number;
  /** What `update_status` answers and `update:changed` last sent. */
  updateStatus: UpdateStatus;
}

export function threadId(view: Pick<ThreadView, 'key'>): string {
  return `${view.key.source}:${view.key.ref}`;
}

export function stateFromSeed(seed: MockSeed): MockState {
  const clone = structuredClone(seed);
  return {
    ...clone,
    nowMs: Date.parse(seed.now),
    tasks: new Map(clone.tasks.map((t) => [t.id, t])),
    threads: new Map(clone.threads.map((t) => [threadId(t.view), t])),
    prs: new Map(clone.prs.map((p) => [p.ref, p])),
    hiddenNeeds: new Set(),
    statusBefore: new Map(),
    nextEventId: Math.max(0, ...clone.events.map((e) => e.id)) + 1,
    nextLinkId: Math.max(0, ...clone.links.map((l) => l.id)) + 1,
    updateStatus: clone.update?.status ?? { state: 'idle' },
  };
}

export const isOpen = (s: Status) => s !== 'done' && s !== 'dropped';

export function ts(ms: number): Timestamp {
  return new Date(ms).toISOString();
}

export function actorOf(text: Actor): Actor {
  return text;
}
