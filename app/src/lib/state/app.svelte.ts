/*
  The main window's controller: owns the stores, talks to Rust through `lib/api`, and holds
  every action the keyboard, menus and components trigger. Components read state from here
  and call its methods; they never call the API themselves.
*/

import { api, on } from '../api';
import type {
  AppError,
  CliStatus,
  CreateTaskInput,
  DoItPlace,
  DoItResult,
  FocusZone,
  LinkDto,
  Mode,
  NavOpen,
  NeedsItem,
  Patch,
  PlanCard,
  NotificationPermission,
  RepoId,
  RowDto,
  RunMap,
  Settings,
  Status,
  T3ProjectRow,
  TaskId,
  ThreadKey,
  ToastEvent,
  UndoableWrite,
} from '../types';
import { tick as svelteTick } from 'svelte';
import { now as clockNow } from '../logic/clock';
import { focusables, nextFocusable } from '../components/trap';
import type { CommandId } from '../logic/commands';
import { controlOwnsKey, zoneOf } from '../logic/focus';
import { keyOf, Sequencer } from '../logic/keymap';
import { resolve } from '../logic/commands';
import { applyAccent, applyAppearance } from '../logic/theme';
import { launchToast } from '../logic/launch';
import { launcherMissingText } from '../logic/connections';
import { statusToast } from '../logic/copy';
import { canMarkDone } from '../logic/needs';
import { keepMapSelection, keepSelection, reveal as revealPlan, selectionAfterFold } from '../logic/selection';
import {
  isListView,
  isTreeLayout,
  isView,
  parentsList,
  sidebarCounts,
  visibleItems,
  viewCount,
  viewTitle,
  type Layout,
  type View,
  type VisibleItem,
} from '../logic/views';
import { ConnectionsStore } from './connections.svelte';
import { DetailStore } from './detail.svelte';
import { NeedsStore } from './needs.svelte';
import { TasksStore } from './tasks.svelte';
import { ToastStore, type ToastAction } from './toasts.svelte';
import { UiStore, type Anchor, type Overlay } from './ui.svelte';

export type ViewKind = 'list' | 'needs' | 'plans' | 'page';

/** Commands a quick repeat of the last key must not run on whatever is selected next. */
const SETTLE_GUARDED = new Set<CommandId>(['rename', 'needs_main', 'needs_dismiss', 'needs_done', 'plans_open']);

const DEFAULT_SETTINGS: Settings = {
  hotkey: 'Control+Alt+N',
  open_at_login: true,
  appearance: 'system',
  notifications: true,
  folded: [],
  start_agents_in: 'auto',
  herdr_agent: 'claude',
};

export function asAppError(e: unknown): AppError {
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) return e as AppError;
  return { code: 'internal', message: 'Something went wrong inside Airboss; details are in the log.', detail: String(e) };
}

export class App {
  readonly tasks = new TasksStore();
  readonly needs = new NeedsStore();
  readonly conn = new ConnectionsStore();
  readonly ui = new UiStore();
  readonly detail = new DetailStore();
  readonly toasts = new ToastStore();

  settings = $state.raw<Settings>(DEFAULT_SETTINGS);
  cli = $state.raw<CliStatus>({ installed: false, path: '~/.local/bin/airboss', on_path: true });
  ready = $state(false);
  fatal = $state.raw<AppError | null>(null);
  /** The database broke after start (R2), rather than failing to open. */
  fatalLost = $state(false);
  now = $state(clockNow());
  plans = $state.raw<PlanCard[]>([]);
  plansLoaded = $state(false);
  runMap = $state.raw<RunMap | null>(null);
  /**
   * Native app (Tauri): real traffic lights and drag regions. Reactive, and given at construction,
   * because the window is mounted before `start()` runs.
   */
  native = $state(false);

  constructor(opts: { native?: boolean } = {}) {
    this.native = opts.native ?? false;
  }

  /** Rows the current view shows (locally hidden rows left out). */
  visible: VisibleItem[] = $derived.by(() => {
    const items = visibleItems(this.tasks.rows, this.tasks.order, this.ui.view, this.ui.folded, this.ui.layout);
    const hidden = this.tasks.hidden;
    return hidden.size ? items.filter((i) => i.kind !== 'row' || !hidden.has(i.id)) : items;
  });
  visibleIds: TaskId[] = $derived(this.visible.flatMap((i) => (i.kind === 'row' ? [i.id] : [])));
  counts = $derived(sidebarCounts(this.tasks.rows));
  parents = $derived(parentsList(this.tasks.rows, this.tasks.order));
  title = $derived(viewTitle(this.ui.view, this.tasks.rows, this.tasks.repos));
  headerCount = $derived(isListView(this.ui.view) ? viewCount(this.tasks.rows, this.tasks.order, this.ui.view) : 0);
  firstRun = $derived(this.ready && this.tasks.rows.size === 0);
  viewKind: ViewKind = $derived(
    this.ui.pageOpen ? 'page' : this.ui.view === 'needs' ? 'needs' : this.ui.view === 'plans' ? 'plans' : 'list',
  );
  /** The Needs me item that's selected, or the first. */
  needsItem = $derived(this.needs.visible.find((n) => n.key === this.needs.selectedKey) ?? null);

  private foldTimer: ReturnType<typeof setTimeout> | null = null;
  private foldPending = new Map<TaskId, boolean>();
  private plansTimer: ReturnType<typeof setTimeout> | null = null;
  private plansReq = 0;
  private mapReq = 0;
  private contextFrame = 0;
  private needsIndex = 0;
  /** Undo entries that hid things locally, to bring them back on undo. */
  private undoLocal = new Map<number, { key?: string; rows?: TaskId[] }>();
  /** Undo entry → the tasks to select again once it's undone. */
  private undoTasks = new Map<number, TaskId[]>();
  /**
   * The undo entries this window was given, oldest first. ⌘Z names the newest one to Rust, so the
   * UI always knows exactly which entry was undone and reverses only its local changes.
   */
  private undoOrder: number[] = [];
  /** SPEC-app: a reject is written 5 s after it’s made, and can be undone until then (tests shorten it). */
  static rejectDelayMs = 5000;
  /** Rust keeps 20 entries; older ones drop off. */
  static readonly UNDO_DEPTH = 20;
  private stopTheme: () => void = () => {};
  private unlisten: (() => void)[] = [];
  focusTargets: Partial<Record<'list' | 'panel' | 'sidebar' | 'page', HTMLElement>> = $state({});

  // ================================================================== start-up

  async start(native: boolean, initial?: { view?: string | null; select?: number | null }): Promise<void> {
    this.native = native;
    try {
      const b = await api.bootstrap();
      this.tasks.load(b);
      this.needs.load(b.needs);
      this.conn.set(b.connections, b.availability, b.agent_report, b);
      this.settings = b.settings;
      this.cli = b.cli;
      this.conn.cliInstalled = b.cli.installed;
      this.conn.agents = b.agents;
      this.ui.folded = new Set(b.settings.folded);
      applyAccent(document.documentElement, b.accent);
      this.applyAppearance();
      this.needs.selectedKey = this.needs.visible[0]?.key ?? null;
      if (this.tasks.rows.size === 0) this.ui.view = 'all';
      if (initial?.view && isView(initial.view)) this.ui.view = initial.view;
      if (initial?.select != null && this.tasks.rows.has(initial.select)) {
        this.ui.selected = initial.select;
        this.ui.panelOpen = true;
        this.loadDetail(initial.select);
      }
      if (this.ui.view === 'plans') this.loadPlans();
      if (this.ui.view === 'needs' && this.needsItem) this.loadDetail(this.needsItem.task);
    } catch (e) {
      this.fatal = asAppError(e);
      this.applyAppearance();
      this.ready = true;
      return;
    }
    this.unlisten = await Promise.all([
      on('tree:patch', (p) => this.onPatch(p)),
      on('needs:changed', (c) => {
        const before = this.needs.visible.map((n) => n.key);
        if (this.needs.apply(c)) this.reconcileNeeds(before);
      }),
      on('connections:changed', (c) => {
        this.conn.set(c.connections, c.availability, c.agent_report, c);
        this.cli = c.cli;
        this.conn.cliInstalled = c.cli.installed;
        this.conn.agents = c.agents;
      }),
      on('menu:action', ({ command }) => this.runCommand(command as CommandId)),
      on('nav:open', (n) => this.navOpen(n)),
      on('toast', (t) => this.toastEvent(t)),
      on('appearance:changed', (a) => applyAccent(document.documentElement, a.accent)),
      on('error:background', (e) => (this.conn.background = e.message)),
      on('db:unavailable', (e) => this.lost(e)),
    ]);
    const tick = () => (this.now = this.conn.now = clockNow());
    setInterval(() => {
      if (document.visibilityState === 'visible') tick();
    }, 60_000);
    document.addEventListener('visibilitychange', tick);
    this.ready = true;
    // Focus starts on the view's list (the inbox on Needs me), never on <body>.
    void svelteTick().then(() => this.focusHome());
  }

  stop(): void {
    for (const u of this.unlisten) u();
    this.stopTheme();
  }

  applyAppearance(): void {
    this.stopTheme();
    this.stopTheme = applyAppearance(document.documentElement, this.settings.appearance);
  }

  // ================================================================== patches

  onPatch(p: Patch): void {
    const selected = this.ui.selected;
    if (!this.tasks.apply(p)) return;
    this.reconcileSelection();
    const upserted = new Set(p.upserts.map((r) => r.id));
    const shown = this.ui.view === 'needs' ? (this.needsItem?.task ?? null) : this.ui.selected;
    if (shown !== null && upserted.has(shown) && (this.ui.panelOpen || this.ui.pageOpen || this.ui.view === 'needs')) {
      this.loadDetail(shown);
    }
    if (selected !== null && p.removed.includes(selected)) {
      this.toast(`Task #${selected} no longer exists.`, 'warn');
    }
    if (this.ui.pageOpen && this.ui.selected !== null && upserted.has(this.ui.selected)) this.loadRunMap(this.ui.selected);
    if (this.ui.view === 'plans' && p.upserts.some((r) => r.parent === null && r.has_children)) {
      if (this.plansTimer) clearTimeout(this.plansTimer);
      this.plansTimer = setTimeout(() => this.loadPlans(), 100);
    }
  }

  /** The same-index rule, after anything that can change the visible list. */
  reconcileSelection(): void {
    if (!isListView(this.ui.view) || this.ui.pageOpen) return;
    const before = this.ui.selected;
    const { selected, index } = keepSelection(this.ui.selected, this.ui.selectedIndex, this.visibleIds);
    this.ui.selectedIndex = index;
    if (selected === before) return;
    this.ui.selected = selected;
    this.ui.mapSel = null;
    if (selected === null) {
      this.ui.panelOpen = false;
      this.detail.clear();
    } else if (this.ui.panelOpen) {
      this.loadDetail(selected);
    }
  }

  private reconcileNeeds(before: string[]): void {
    const keys = this.needs.visible.map((n) => n.key);
    const prevIndex = Math.max(0, before.indexOf(this.needs.selectedKey ?? ''));
    const { selected } = keepSelection(this.needs.selectedKey ?? keys[0] ?? null, prevIndex, keys);
    const changed = selected !== this.needs.selectedKey;
    this.needs.selectedKey = selected;
    if (changed && this.ui.view === 'needs' && this.needsItem) this.loadDetail(this.needsItem.task);
  }

  // ================================================================== loading

  loadDetail(id: TaskId): void {
    const req = this.detail.begin(id);
    api
      .taskDetail(id, req, this.detail.limit)
      .then((d) => this.detail.accept(d))
      .catch((e) => this.detail.fail(req, asAppError(e).message));
  }

  showMoreHistory(): void {
    const id = this.detail.current;
    if (id === null) return;
    this.detail.limit = 200;
    this.loadDetail(id);
  }

  loadRunMap(id: TaskId): void {
    const req = ++this.mapReq;
    api
      .runMap(id, req)
      .then((m) => {
        if (req !== this.mapReq || this.ui.selected !== id) return;
        this.runMap = m;
        this.ui.mapSel = keepMapSelection(this.ui.mapSel, m.waves.flat());
      })
      .catch(() => {});
  }

  loadPlans(): void {
    const req = ++this.plansReq;
    api
      .plans(req)
      .then((cards) => {
        if (req !== this.plansReq) return;
        this.plans = cards;
        this.plansLoaded = true;
        this.ui.planIndex = Math.min(this.ui.planIndex, Math.max(0, cards.length - 1));
      })
      .catch(() => {});
  }

  // ================================================================== navigation

  go(view: View): void {
    if (!isView(view)) return;
    this.ui.view = view;
    this.ui.pageOpen = false;
    this.ui.mapSel = null;
    this.ui.renaming = null;
    if (view === 'plans') this.loadPlans();
    if (view === 'needs') {
      if (!this.needsItem) this.needs.selectedKey = this.needs.visible[0]?.key ?? null;
      if (this.needsItem) this.loadDetail(this.needsItem.task);
    }
    if (isListView(view)) {
      this.reconcileSelection();
      if (this.ui.selected !== null && this.ui.panelOpen) this.loadDetail(this.ui.selected);
    }
    this.focus('list');
  }

  setLayout(layout: Layout): void {
    this.ui.layout = layout;
    this.reconcileSelection();
  }

  /** Selects a row; the panel follows when it's open (or when asked to open). */
  select(id: TaskId | null, opts: { open?: boolean; keyboard?: boolean } = {}): void {
    if (id === null) return;
    const changed = this.ui.selected !== id;
    this.ui.selected = id;
    this.ui.selectedIndex = this.visibleIds.indexOf(id);
    this.ui.keyboard = opts.keyboard ?? false;
    if (changed) {
      this.ui.mapSel = null;
      this.ui.renaming = null;
    }
    if (opts.open) this.ui.panelOpen = true;
    if ((changed || opts.open) && (this.ui.panelOpen || this.ui.pageOpen)) this.loadDetail(id);
  }

  moveSelection(delta: number | 'first' | 'last'): void {
    const ids = this.visibleIds;
    if (!ids.length) return;
    let i = this.ui.selected === null ? -1 : ids.indexOf(this.ui.selected);
    if (delta === 'first') i = 0;
    else if (delta === 'last') i = ids.length - 1;
    else i = i === -1 ? (delta > 0 ? 0 : ids.length - 1) : Math.max(0, Math.min(ids.length - 1, i + delta));
    this.select(ids[i]!, { keyboard: true });
  }

  openPanel(id: TaskId | null = this.ui.selected): void {
    if (id === null) return;
    this.ui.panelOpen = true;
    this.select(id, { open: true, keyboard: this.ui.keyboard });
  }

  closePanel(): void {
    this.ui.panelOpen = false;
    this.focus('list');
  }

  openPage(id: TaskId | null = this.ui.selected): void {
    if (id === null || !this.tasks.rows.has(id)) return;
    if (!isListView(this.ui.view) || !this.visibleIds.includes(id)) this.revealOnly(id);
    this.select(id, { keyboard: true });
    this.ui.pageOpen = true;
    this.ui.mapSel = null;
    this.runMap = null;
    this.loadDetail(id);
    if (this.tasks.rows.get(id)?.has_children) this.loadRunMap(id);
    this.focus('page');
  }

  closePage(): void {
    this.ui.pageOpen = false;
    this.ui.mapSel = null;
    if (this.ui.panelOpen && this.ui.selected !== null) this.loadDetail(this.ui.selected);
    this.focus('list');
  }

  /** Esc: close the page; on the page, a map selection clears first; else close the panel. */
  escape(): void {
    if (this.ui.pageOpen && this.ui.mapSel !== null) this.ui.mapSel = null;
    else if (this.ui.pageOpen) this.closePage();
    else if (this.ui.panelOpen) this.closePanel();
  }

  private revealOnly(id: TaskId): void {
    const plan = revealPlan(this.tasks.rows, this.tasks.order, this.ui.view, id);
    if (plan.unfold.length) this.setFolded(plan.unfold, false);
    if (plan.view !== this.ui.view) {
      this.ui.view = plan.view;
      this.ui.pageOpen = false;
    }
  }

  /** Jump, the menu bar and toasts: unfold ancestors, switch view if needed, open the panel. */
  reveal(id: TaskId): void {
    if (!this.tasks.rows.has(id)) return;
    this.revealOnly(id);
    this.ui.pageOpen = false;
    this.select(id, { open: true, keyboard: true });
    this.focus('list');
  }

  navOpen(n: NavOpen): void {
    if (n.needs_key) {
      this.go('needs');
      this.selectNeed(n.needs_key);
    } else if (n.task !== null) {
      this.reveal(n.task);
    } else if (n.view && isView(n.view)) {
      this.go(n.view);
    }
  }

  focus(target: 'list' | 'panel' | 'sidebar' | 'page'): void {
    queueMicrotask(() => {
      const el = this.focusTargets[target] ?? (target === 'page' ? this.focusTargets.panel : undefined);
      el?.focus({ preventScroll: true });
    });
  }

  // ================================================================== folds

  isFolded(id: TaskId): boolean {
    return this.ui.folded.has(id);
  }

  toggleFold(id: TaskId): void {
    this.setFolded([id], !this.ui.folded.has(id));
  }

  setFolded(ids: TaskId[], folded: boolean): void {
    const next = new Set(this.ui.folded);
    for (const id of ids) {
      if (folded) next.add(id);
      else next.delete(id);
      this.foldPending.set(id, folded);
    }
    this.ui.folded = next;
    if (folded && ids.length === 1) {
      const sel = selectionAfterFold(this.tasks.rows, this.ui.selected, ids[0]!);
      if (sel !== this.ui.selected) this.select(sel, { keyboard: this.ui.keyboard });
    }
    this.reconcileSelection();
    if (this.foldTimer) clearTimeout(this.foldTimer);
    this.foldTimer = setTimeout(() => this.flushFolds(), 1000);
  }

  private flushFolds(): void {
    const on = [...this.foldPending].filter(([, f]) => f).map(([id]) => id);
    const off = [...this.foldPending].filter(([, f]) => !f).map(([id]) => id);
    this.foldPending.clear();
    if (on.length) void api.setFolded(on, true).catch(() => {});
    if (off.length) void api.setFolded(off, false).catch(() => {});
  }

  /** → on a row: open a folded parent, or go to its first subtask. */
  arrowRight(): void {
    const r = this.tasks.get(this.ui.selected);
    if (!r || !r.has_children || !isTreeLayout(this.ui.view, this.ui.layout)) return;
    if (this.ui.folded.has(r.id)) this.setFolded([r.id], false);
    else {
      const i = this.visibleIds.indexOf(r.id);
      const next = this.visibleIds[i + 1];
      if (next !== undefined && this.tasks.rows.get(next)?.parent === r.id) this.select(next, { keyboard: true });
    }
  }

  /** ← on a row: fold an open parent, or go up to the parent. */
  arrowLeft(): void {
    const r = this.tasks.get(this.ui.selected);
    if (!r || !isTreeLayout(this.ui.view, this.ui.layout)) return;
    if (r.has_children && !this.ui.folded.has(r.id)) this.setFolded([r.id], true);
    else if (r.parent !== null && this.visibleIds.includes(r.parent)) this.select(r.parent, { keyboard: true });
  }

  // ================================================================== overlays

  open(o: Overlay): void {
    if (!this.ui.overlay) this.ui.returnFocus = this.pendingReturn ?? (document.activeElement as HTMLElement | null);
    this.pendingReturn = null;
    this.ui.overlay = o;
  }

  /** Focus to give back once the overlay that just closed is gone (unless another opens first). */
  private pendingReturn: HTMLElement | null = null;

  close(): void {
    this.ui.overlay = null;
    const el = this.ui.returnFocus;
    this.ui.returnFocus = null;
    this.pendingReturn = el;
    queueMicrotask(() => {
      // A menu item that opens another overlay (Blocked, Drop) hands its focus target on.
      if (this.ui.overlay || this.pendingReturn !== el) return;
      this.pendingReturn = null;
      // Give focus back to a real element: never <body>.
      if (el && el.isConnected && el !== document.body && !el.closest('[data-zone="overlay"]')) el.focus({ preventScroll: true });
      else this.focusHome();
    });
  }

  /** Where focus goes when nothing better is known: the page, or the view's list. */
  focusHome(): void {
    this.focus(this.ui.pageOpen ? 'page' : 'list');
  }

  /** Esc in a field that doesn't handle it: leave it for its panel or list, not for <body>. */
  leaveField(el: HTMLElement): void {
    const zone = el.closest('[data-zone="panel"]') ? 'panel' : null;
    el.blur();
    if (zone) this.focus(this.ui.pageOpen ? 'page' : 'panel');
    else this.focusHome();
  }

  /** Where a menu for this task opens: under its status icon, its row, or the middle. */
  anchor(id: TaskId, what: 'status' | 'row' | 'do' = 'status'): Anchor {
    const sel =
      what === 'do'
        ? '[data-do-it]'
        : what === 'status'
          ? `[data-status-for="${id}"]`
          : `[data-row="${id}"]`;
    const el = document.querySelector(sel) ?? document.querySelector(`[data-row="${id}"]`);
    if (el) {
      const r = el.getBoundingClientRect();
      if (r.height > 0) return { x: what === 'row' ? r.left + 60 : r.left, y: r.bottom + 4, w: r.width };
    }
    return { x: Math.round(window.innerWidth / 2 - 110), y: Math.round(window.innerHeight * 0.3) };
  }

  openNew(opts: { plan?: boolean } = {}): void {
    const { kind, id } = parseViewKind(this.ui.view);
    this.open({
      kind: 'new',
      parent: kind === 'parent' ? id : null,
      repos: kind === 'repo' && id !== null ? [id] : [],
      plan: opts.plan ?? false,
    });
  }

  // ================================================================== toasts and errors

  toast(text: string, tone: 'info' | 'warn' = 'info', actions: ToastAction[] = [], detail?: string): void {
    this.toasts.push({ text, tone, actions, ...(detail ? { detail } : {}) });
  }

  /** The database broke while the app ran: the full-window state, until Airboss is quit. */
  lost(e: AppError): void {
    if (this.fatal) return;
    this.fatalLost = true;
    this.fatal = e;
  }

  fail(e: unknown): void {
    const err = asAppError(e);
    // Appendix D: no toast; the button already says "Starting…".
    if (err.code === 'already_starting') return;
    if (err.code === 'db_unavailable' || err.code === 'db_too_new') return this.lost(err);
    // The app that would open a link isn't verified (Rust's handlers.rs): offer the link.
    if (err.code === 'app_unverified' && err.detail) {
      const link = err.detail;
      return this.toast(err.message, 'warn', [
        { label: 'Copy link', run: () => void api.copyText({ link }).then(() => this.toast('Copied the link'), (e: unknown) => this.fail(e)) },
      ]);
    }
    this.toast(err.message, 'warn');
  }

  /**
   * A toast for an undoable write. `local` is what this window hid for the write (rows, a Needs
   * me item) and must bring back on undo; `tasks` is what to select again once it's undone.
   */
  private undoToast(
    text: string,
    write: UndoableWrite,
    local?: { key?: string; rows?: TaskId[] },
    tasks?: TaskId[],
    deadline?: number,
  ): void {
    const undo = write.undo;
    if (undo) this.rememberUndo(undo.id);
    if (undo && local) this.undoLocal.set(undo.id, local);
    if (undo && tasks?.length) this.undoTasks.set(undo.id, tasks);
    if (undo && deadline !== undefined) {
      // A deferred reject is written at its deadline: its local fix-up and Undo stop being true.
      setTimeout(() => {
        this.forgetUndo(undo.id);
        if (local?.rows) setTimeout(() => this.tasks.hide(local.rows!, false), 2000);
      }, Math.max(0, deadline - Date.now()));
    }
    this.toasts.push(
      {
        text,
        tone: 'info',
        actions: undo ? [{ label: 'Undo', run: () => this.undo(undo.id) }] : [],
        ...(undo ? { undo } : {}),
      },
      deadline,
    );
  }

  /**
   * A toast Rust sent. Each action names a command and its args in the api function's order
   * (dto.rs `ToastArgs`), so it runs as `api[command](...Object.values(args))`.
   */
  private toastEvent(t: ToastEvent): void {
    const fns = api as unknown as Record<string, ((...x: unknown[]) => Promise<unknown>) | undefined>;
    this.toasts.push({
      text: t.text,
      tone: t.tone === 'warning' ? 'warn' : 'info',
      actions: [
        ...t.actions.map((a) => ({
          label: a.label,
          run: () => {
            // Retry on a reject that couldn't be written: the same reject, with its undo window.
            if (a.command === 'reject_proposals') return void this.rejectAgain(a.args.ids as TaskId[]);
            const fn = fns[camel(a.command)];
            if (!fn) return console.error(`toast action: no api function for ${a.command}`);
            fn(...Object.values(a.args)).catch((e: unknown) => this.fail(e));
          },
        })),
        ...(t.undo ? [{ label: 'Undo', run: () => this.undo(t.undo!.id) }] : []),
      ],
    });
    if (t.undo) this.rememberUndo(t.undo.id);
  }

  private rememberUndo(id: number): void {
    this.undoOrder = [...this.undoOrder.filter((x) => x !== id), id].slice(-App.UNDO_DEPTH);
  }

  private forgetUndo(id: number): void {
    this.undoOrder = this.undoOrder.filter((x) => x !== id);
    this.undoLocal.delete(id);
    this.undoTasks.delete(id);
  }

  // ================================================================== writes

  private applyWrite(p: Patch | null | undefined): void {
    if (p) this.onPatch(p);
  }

  async toggleDone(id: TaskId | null = this.actionTask()): Promise<void> {
    const r = this.tasks.get(id);
    if (!r || r.status === 'proposed') return;
    try {
      const w = await api.toggleDone(r.id);
      this.applyWrite(w.patch);
      const now = this.tasks.get(r.id);
      this.undoToast(now?.status === 'done' ? `Marked #${r.id} done` : `Marked #${r.id} not done`, w, undefined, [r.id]);
    } catch (e) {
      this.fail(e);
    }
  }

  /** S, the status icon, ⌘B and the menus all land here. Blocked asks why; Dropped confirms. */
  async setStatus(id: TaskId, status: Status, note: string | null = null): Promise<void> {
    const r = this.tasks.get(id);
    if (!r) return;
    if (status === 'blocked' && !note) return this.open({ kind: 'blocked', task: id, at: this.anchor(id) });
    if (status === 'dropped' && !note) {
      const open = this.openDescendants(id);
      return this.open({ kind: 'drop', task: id, open });
    }
    try {
      const w = await api.setStatus(id, status, status === 'blocked' ? note : null);
      this.applyWrite(w.patch);
      if (w.patch) this.undoToast(statusToast(id, status), w, undefined, [id]);
    } catch (e) {
      this.fail(e);
    }
  }

  async drop(id: TaskId): Promise<void> {
    try {
      const w = await api.setStatus(id, 'dropped', null);
      this.applyWrite(w.patch);
      this.undoToast(`Dropped #${id}`, w, undefined, [id]);
    } catch (e) {
      this.fail(e);
    }
  }

  openDescendants(id: TaskId): number {
    let n = 0;
    const at = this.tasks.order.indexOf(id);
    const depth = this.tasks.rows.get(id)?.depth ?? 0;
    for (let i = at + 1; i < this.tasks.order.length; i++) {
      const r = this.tasks.rows.get(this.tasks.order[i]!)!;
      if (r.depth <= depth) break;
      if (r.status !== 'done' && r.status !== 'dropped') n++;
    }
    return n;
  }

  /** A row's rename ends (↩, blur or Esc): focus goes back to the list, once. */
  endRowRename(id: TaskId, title: string | null): void {
    if (this.ui.renaming !== id) return;
    this.ui.renaming = null;
    this.focus('list');
    if (title !== null) void this.rename(id, title);
  }

  async rename(id: TaskId, title: string): Promise<void> {
    this.ui.renaming = null;
    const r = this.tasks.get(id);
    if (!r || !title.trim() || title.trim() === r.title) return;
    try {
      this.applyWrite(await api.renameTask(id, title.trim()));
    } catch (e) {
      this.fail(e);
    }
  }

  async editNotes(id: TaskId, body: string): Promise<void> {
    try {
      this.applyWrite(await api.editNotes(id, body.trim() ? body : null));
    } catch (e) {
      this.fail(e);
    }
  }

  async addNote(id: TaskId, text: string): Promise<boolean> {
    if (!text.trim()) return false;
    try {
      this.applyWrite(await api.addNote(id, text.trim()));
      return true;
    } catch (e) {
      this.fail(e);
      return false;
    }
  }

  async toggleRepo(id: TaskId, repo: RepoId, link: boolean): Promise<void> {
    try {
      this.applyWrite(link ? await api.linkRepo(id, repo) : await api.unlinkRepo(id, repo));
    } catch (e) {
      this.fail(e);
    }
  }

  /** Returns an error message to show inline, or null. */
  async setBranch(id: TaskId, branch: string): Promise<string | null> {
    try {
      this.applyWrite(await api.setBranch(id, branch));
      return null;
    } catch (e) {
      return asAppError(e).message;
    }
  }

  async addDependency(id: TaskId, on: TaskId): Promise<void> {
    try {
      this.applyWrite(await api.addDependency(id, on));
      this.toast(`Made #${id} wait on #${on}`);
    } catch (e) {
      this.fail(e);
    }
  }

  async removeDependency(id: TaskId, on: TaskId): Promise<void> {
    try {
      this.applyWrite(await api.removeDependency(id, on));
    } catch (e) {
      this.fail(e);
    }
  }

  /** Returns an error message to show inline, or null. */
  async addLink(id: TaskId, text: string, title: string | null): Promise<string | null> {
    try {
      this.applyWrite(await api.addLink(id, text.trim(), title?.trim() || null));
      return null;
    } catch (e) {
      return asAppError(e).message;
    }
  }

  async removeLink(link: number): Promise<void> {
    try {
      this.applyWrite(await api.removeLink(link));
    } catch (e) {
      this.fail(e);
    }
  }

  async moveUnder(id: TaskId, parent: TaskId | null): Promise<void> {
    try {
      const w = await api.moveTask(id, parent, null);
      this.applyWrite(w.patch);
      this.undoToast(parent === null ? `Moved #${id} to the top level` : `Moved #${id} under #${parent}`, w, undefined, [id]);
      this.reveal(id);
    } catch (e) {
      this.fail(e);
    }
  }

  async reorder(direction: 'up' | 'down'): Promise<void> {
    const id = this.ui.selected;
    if (id === null) return;
    if (!isTreeLayout(this.ui.view, this.ui.layout)) return this.toast('Reorder in All tasks or a parent’s view');
    try {
      const w = await api.reorder(id, direction);
      this.applyWrite(w.patch);
      if (w.patch) this.undoToast(`Moved #${id} ${direction}`, w, undefined, [id]);
    } catch (e) {
      this.fail(e);
    }
  }

  async copyNumber(id: TaskId | null = this.ui.selected): Promise<void> {
    if (id === null) return;
    try {
      await api.copyText({ task_number: id });
      this.toast(`Copied #${id}`);
    } catch (e) {
      this.fail(e);
    }
  }

  openLink(l: LinkDto): void {
    // A Herdr terminal opens by its address, whether or not sync has reported it yet.
    const key: ThreadKey | null = l.thread?.key ?? (l.kind === 'herdr_terminal' ? { source: 'herdr', ref: l.ref } : null);
    const p = key ? api.openThread(key) : api.openUrl({ link: l.id });
    p.catch((e) => this.fail(e));
  }

  openThread(key: ThreadKey): void {
    api.openThread(key).catch((e) => this.fail(e));
  }

  /** ⌘O: the latest thread or PR. */
  async openLatest(id: TaskId | null = this.ui.selected): Promise<void> {
    if (id === null) return;
    let links = this.detail.detail?.task.id === id ? this.detail.detail.links : null;
    if (!links) {
      try {
        links = (await api.taskDetail(id, 0, 30)).links;
      } catch (e) {
        return this.fail(e);
      }
    }
    const candidates = links
      .filter((l) => (l.thread && (l.thread.url || l.kind === 't3_thread' || l.kind === 'herdr_terminal')) || (l.kind === 'pr' && (l.url || l.pr?.url)))
      .sort((a, b) => b.linked_at.localeCompare(a.linked_at));
    const latest = candidates[0];
    if (!latest) return this.toast(`#${id} has no thread or PR yet`);
    try {
      if (latest.thread) await api.openThread(latest.thread.key);
      else await api.openUrl({ link: latest.id });
    } catch (e) {
      this.fail(e);
    }
  }

  // ================================================================== Do it and Plan it

  /** The task ⌘↩ acts on: the selected map station on the page, else the selection. */
  actionTask(): TaskId | null {
    if (this.ui.view === 'needs' && !this.ui.pageOpen) return this.needsItem?.task ?? null;
    if (this.ui.pageOpen && this.ui.mapSel !== null) return this.ui.mapSel;
    return this.ui.selected;
  }

  async doIt(id: TaskId | null = this.actionTask(), place: DoItPlace | null = null, again = false): Promise<void> {
    await this.launch(id, 'do_it', place, again);
  }

  async planIt(id: TaskId | null = this.actionTask(), again = false): Promise<void> {
    await this.launch(id, 'plan_it', null, again);
  }

  private async launch(id: TaskId | null, mode: Mode, place: DoItPlace | null, again: boolean): Promise<void> {
    if (id === null || !this.tasks.rows.has(id)) return;
    if (!this.conn.launchReady) {
      return this.toast(this.conn.launchMissing ?? launcherMissingText(this.conn.launcher), 'warn');
    }
    if (this.ui.starting.has(id)) return;
    this.ui.setStarting(id, true);
    try {
      const res: DoItResult = mode === 'do_it' ? await api.doIt(id, place, again) : await api.planIt(id, again);
      this.launched(id, mode, res, { place, again });
    } catch (e) {
      const err = asAppError(e);
      if (err.code === 'task_closed') {
        this.toast(err.message, 'warn', [{ label: 'Reopen and start', run: () => void this.reopenAndStart(id) }]);
      } else this.fail(err);
    } finally {
      this.ui.setStarting(id, false);
    }
  }

  private launched(id: TaskId, mode: Mode, res: DoItResult, req: { place: DoItPlace | null; again: boolean }): void {
    if ('needs_choice' in res) {
      const inPanel = this.ui.panelOpen || this.ui.pageOpen;
      this.open({
        kind: 'start_in',
        task: id,
        at: this.anchor(id, inPanel ? 'do' : 'row'),
        choices: res.needs_choice.choices,
        researchDir: res.needs_choice.research_dir,
      });
      return;
    }
    const { report, patch } = res.started;
    this.applyWrite(patch);
    const t = launchToast(id, mode, report, this.tasks.repoById);
    const actions: ToastAction[] = t.actions.map((a) => {
      switch (a.kind) {
        case 'open_thread':
          return { label: a.label, run: () => void api.openThread(a.key).catch((e) => this.fail(e)) };
        case 'retry':
          return { label: a.label, run: () => void this.launch(id, mode, req.place, req.again) };
        case 'again':
          return { label: a.label, run: () => void this.launch(id, mode, req.place, true) };
        case 'add_to_t3':
          return {
            label: a.label,
            run: () =>
              void api
                .addToT3(a.repo)
                .then(() => this.launch(id, mode, req.place, req.again))
                .catch((e) => this.fail(e)),
          };
      }
    });
    this.toast(t.text, t.tone, actions);
  }

  async reopenAndStart(id: TaskId): Promise<void> {
    this.ui.setStarting(id, true);
    try {
      this.launched(id, 'do_it', await api.reopenAndStart(id), { place: null, again: false });
    } catch (e) {
      this.fail(e);
    } finally {
      this.ui.setStarting(id, false);
    }
  }

  // ================================================================== Needs me

  selectNeed(key: string | null, keyboard = true): void {
    if (key === null) return;
    // The user chose an item: the settle after the last action is over.
    this.settleUntil = 0;
    const changed = this.needs.selectedKey !== key;
    this.needs.selectedKey = key;
    this.ui.keyboard = keyboard;
    this.needsIndex = this.needs.visible.findIndex((n) => n.key === key);
    if (changed && this.needsItem) this.loadDetail(this.needsItem.task);
  }

  moveNeed(delta: number): void {
    const list = this.needs.visible.filter((n) => !this.needs.leaving.has(n.key));
    if (!list.length) return;
    const i = list.findIndex((n) => n.key === this.needs.selectedKey);
    const next = list[i === -1 ? 0 : Math.max(0, Math.min(list.length - 1, i + delta))]!;
    this.selectNeed(next.key);
  }

  /**
   * After an action, keys that run an item's action (↩, ⌘↩, ⌘⌫) and a rename wait this long, or
   * until the user moves the selection, so a quick second press can't act on the next item.
   */
  private settleUntil = 0;
  static readonly SETTLE_MS = 400;

  settle(): void {
    this.settleUntil = Date.now() + App.SETTLE_MS;
  }

  settling(): boolean {
    return Date.now() < this.settleUntil;
  }

  /** One action per item per settle. */
  private actOnce(item: NeedsItem): boolean {
    if (this.settling() || this.needs.leaving.has(item.key) || this.needs.hidden.has(item.key)) return false;
    this.settle();
    return true;
  }

  /** Hides the item at once (180 ms exit), then the selection moves to the item at the same index. */
  private leave(item: NeedsItem, ms?: number): void {
    const before = this.needs.visible.map((n) => n.key);
    this.needs.hideLocally(item.key, ms, () => this.reconcileNeeds(before));
  }

  async mainAction(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item || !this.actOnce(item)) return;
    try {
      switch (item.kind) {
        case 'question':
        case 'approval':
        case 'failed_run':
        case 'agent_stopped':
          if ('thread' in item.target) await api.openThread(item.target.thread);
          this.leave(item);
          break;
        case 'review':
          if ('pr' in item.target) await api.openUrl(item.target.pr.url ? { url: item.target.pr.url } : { url: `https://github.com/${item.target.pr.ref.replace('#', '/pull/')}` });
          this.leave(item);
          break;
        case 'proposed':
          await this.acceptProposals(item, item.proposed.map(([id]) => id));
          break;
        case 'ready':
          this.leave(item);
          await this.doIt(item.task);
          break;
        case 'finish_it': {
          this.leave(item);
          const w = await api.setStatus(item.task, 'done', null);
          this.applyWrite(w.patch);
          this.undoToast(`Marked #${item.task} done`, w);
          break;
        }
        case 'quiet':
          this.reveal(item.task);
          break;
      }
    } catch (e) {
      this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  /** Mark done (Space) on a kind that offers it: Agent stopped, Finish it?. As me, with Undo. */
  async markNeedDone(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item || !canMarkDone(item.kind)) return;
    if (item.kind === 'finish_it') return this.mainAction(item);
    await this.setNeedStatus(item, 'done');
  }

  /** Agent stopped's "Set in review": the work is done and waits on a review outside Airboss. */
  async setNeedInReview(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item || item.kind !== 'agent_stopped') return;
    await this.setNeedStatus(item, 'in_review');
  }

  private async setNeedStatus(item: NeedsItem, status: 'done' | 'in_review'): Promise<void> {
    if (!this.actOnce(item)) return;
    this.leave(item);
    try {
      const w = await api.setStatus(item.task, status, null);
      this.applyWrite(w.patch);
      this.undoToast(statusToast(item.task, status), w);
    } catch (e) {
      this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  /** Dismiss a Needs me item; on Finish it? this is Keep open. */
  async dismiss(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item || !this.actOnce(item)) return;
    this.leave(item);
    try {
      const w = await api.needsDismiss(item.key);
      this.undoToast(item.kind === 'finish_it' ? `Kept #${item.task} open` : `Dismissed #${item.task}`, w, { key: item.key });
    } catch (e) {
      this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  async snooze(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item || !this.actOnce(item)) return;
    this.leave(item);
    try {
      const w = await api.needsSnooze(item.key, null);
      this.undoToast(`Snoozed #${item.task} for 3 days`, w, { key: item.key });
    } catch (e) {
      this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  async acceptProposals(item: NeedsItem, ids: TaskId[]): Promise<void> {
    const all = ids.length === item.proposed.length;
    if (all) this.leave(item);
    try {
      this.applyWrite(await api.acceptProposals(ids));
      this.toast(ids.length === 1 ? `Accepted #${ids[0]}` : `Added ${ids.length} subtasks to #${item.task}`);
    } catch (e) {
      if (all) this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  /** Edit a proposal's title, then accept it. */
  async editAndAccept(item: NeedsItem, id: TaskId, title: string): Promise<void> {
    try {
      if (title.trim()) this.applyWrite(await api.renameTask(id, title.trim()));
      await this.acceptProposals(item, [id]);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Reject commits after a 5 s undo window; until then the item and rows are hidden here. */
  async rejectProposals(item: NeedsItem, ids: TaskId[]): Promise<void> {
    const all = ids.length >= item.proposed.filter(([id]) => !this.tasks.hidden.has(id)).length;
    if (all && !this.actOnce(item)) return;
    this.tasks.hide(ids, true);
    if (all) this.leave(item, 7000);
    // Rust starts its 5 s once the call arrives, so counting from here never outlives it.
    const deadline = Date.now() + App.rejectDelayMs;
    try {
      const w = await api.rejectProposals(ids, null);
      this.undoToast(
        ids.length === 1 ? `Rejected #${ids[0]}` : `Rejected ${ids.length} subtasks`,
        w,
        { rows: ids, ...(all ? { key: item.key } : {}) },
        undefined,
        deadline,
      );
    } catch (e) {
      this.tasks.hide(ids, false);
      if (all) this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  /** Accept or reject one proposal from anywhere (the status menu of a proposed task). */
  async acceptOne(id: TaskId): Promise<void> {
    const item = this.needs.visible.find((n) => n.kind === 'proposed' && n.proposed.some(([p]) => p === id));
    if (item) return this.acceptProposals(item, [id]);
    try {
      this.applyWrite(await api.acceptProposals([id]));
      this.toast(`Accepted #${id}`);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Retry on a reject Rust couldn't write: hide the rows again and reject them with Undo. */
  async rejectAgain(ids: TaskId[]): Promise<void> {
    if (ids.length === 1) return this.rejectOne(ids[0]!);
    const item = this.needs.visible.find((n) => n.kind === 'proposed' && ids.every((id) => n.proposed.some(([p]) => p === id)));
    if (item) return this.rejectProposals(item, ids);
    this.tasks.hide(ids, true);
    const deadline = Date.now() + App.rejectDelayMs;
    try {
      const w = await api.rejectProposals(ids, null);
      this.undoToast(`Rejected ${ids.length} subtasks`, w, { rows: ids }, undefined, deadline);
    } catch (e) {
      this.tasks.hide(ids, false);
      this.fail(e);
    }
  }

  async rejectOne(id: TaskId): Promise<void> {
    const item = this.needs.visible.find((n) => n.kind === 'proposed' && n.proposed.some(([p]) => p === id));
    if (item) return this.rejectProposals(item, [id]);
    this.tasks.hide([id], true);
    const deadline = Date.now() + App.rejectDelayMs;
    try {
      const w = await api.rejectProposals([id], null);
      this.undoToast(`Rejected #${id}`, w, { rows: [id] }, undefined, deadline);
    } catch (e) {
      this.tasks.hide([id], false);
      this.fail(e);
    }
  }

  // ================================================================== undo

  /**
   * ⌘Z (no entry) undoes the newest entry this window knows, named explicitly, so the local
   * fix-up below is for exactly the entry Rust undid. A toast's Undo names its own entry.
   */
  async undo(entry: number | null = null): Promise<void> {
    const id = entry ?? this.undoOrder.at(-1) ?? null;
    try {
      const res = await api.undo(id);
      this.applyWrite(res.patch);
      const local = id !== null ? this.undoLocal.get(id) : undefined;
      const tasks = id !== null ? this.undoTasks.get(id) : undefined;
      if (id !== null) this.forgetUndo(id);
      if (local?.key) this.needs.unhide(local.key);
      if (local?.rows) this.tasks.hide(local.rows, false);
      // The task comes back where it was: select it again if this view shows it.
      const back = tasks?.find((t) => this.visibleIds.includes(t));
      if (back !== undefined && isListView(this.ui.view)) this.select(back, { keyboard: true });
      this.toast(res.message);
    } catch (e) {
      this.fail(e);
    }
  }

  // ================================================================== create

  /** Creates a task; Do it and Plan it then run through the normal flow (with Starting…). */
  async create(input: CreateTaskInput): Promise<TaskId | null> {
    try {
      const res = await api.createTask({ ...input, then: 'none' });
      this.applyWrite(res.patch);
      this.reveal(res.id);
      this.settle();
      for (const l of res.link_errors) this.toast(`Created #${res.id}, but couldn't link ${l.text}`, 'warn');
      if (input.then === 'do_it') void this.doIt(res.id);
      if (input.then === 'plan_it') void this.planIt(res.id);
      return res.id;
    } catch (e) {
      this.fail(e);
      return null;
    }
  }

  // ================================================================== settings and connections

  async saveSettings(next: Settings): Promise<string | null> {
    try {
      this.settings = await api.setSettings(next);
      this.applyAppearance();
      return null;
    } catch (e) {
      return asAppError(e).message;
    }
  }

  async recheck(): Promise<void> {
    try {
      await api.recheckConnections();
      this.toast('Checking connections…');
    } catch (e) {
      this.fail(e);
    }
  }

  async installCli(): Promise<void> {
    try {
      const r = await api.installCli();
      this.cli = { installed: true, path: r.path, on_path: r.on_path };
      this.conn.cliInstalled = true;
      // It always lands in the home folder's ~/.local/bin: say so, not the long absolute path.
      const at = r.path.replace(/^.*(\/\.local\/bin\/airboss)$/, '~$1');
      this.toast(r.on_path ? `Installed airboss at ${at}` : `Installed at ${at}. Add ~/.local/bin to your PATH.`);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Connect Claude Code (#9): Rust runs `claude mcp add`, and the connections follow. */
  async connectClaude(): Promise<boolean> {
    if (this.conn.connecting) return false;
    this.conn.connecting = true;
    try {
      await api.connectClaude();
      this.toast('Connected Claude Code');
      return true;
    } catch (e) {
      this.toast(asAppError(e).message, 'warn', [{ label: 'Copy setup command', run: () => void this.copy('setup_command') }]);
      return false;
    } finally {
      this.conn.connecting = false;
    }
  }

  async t3Projects(): Promise<T3ProjectRow[]> {
    try {
      return await api.t3Projects();
    } catch (e) {
      this.fail(e);
      return [];
    }
  }

  /** Herdr's space and recent folders, for first run without T3 Code. */
  async herdrFolders(): Promise<T3ProjectRow[]> {
    try {
      return await api.herdrFolders();
    } catch (e) {
      this.fail(e);
      return [];
    }
  }

  async pickFolder(): Promise<string | null> {
    try {
      return await api.pickFolder();
    } catch (e) {
      this.fail(e);
      return null;
    }
  }

  /** Returns an error message to show inline, or null. */
  async addRepos(paths: string[]): Promise<string | null> {
    try {
      const res = await api.addRepos(paths);
      this.applyWrite(res.patch);
      this.toast(res.repos.length === 1 ? `Added ${res.repos[0]!.name}` : `Added ${res.repos.length} repos`);
      return null;
    } catch (e) {
      return asAppError(e).message;
    }
  }

  /** Settings › Repos: the repo as it should be. Returns an error message to show inline, or null. */
  async editRepo(repo: RepoId, name: string, path: string, remote: string | null): Promise<string | null> {
    try {
      this.applyWrite(await api.editRepo(repo, name, path, remote));
      this.toast(`Saved ${name.trim()}`);
      return null;
    } catch (e) {
      const err = asAppError(e);
      if (err.code === 'db_unavailable' || err.code === 'db_too_new') this.lost(err);
      return err.message;
    }
  }

  /** Settings › Repos › Remove, once confirmed: says how many tasks lost the link. */
  async removeRepo(repo: RepoId): Promise<boolean> {
    const name = this.tasks.repos.find((r) => r.id === repo)?.name ?? 'the repo';
    try {
      const res = await api.removeRepo(repo);
      this.applyWrite(res.patch);
      const n = res.unlinked.length;
      this.toast(n ? `Removed ${name}. ${n} ${n === 1 ? 'task' : 'tasks'} lost the link.` : `Removed ${name}`);
      return true;
    } catch (e) {
      this.fail(e);
      return false;
    }
  }

  /** Settings › Database › Back up…: a save dialog, then a copy. */
  async backUp(): Promise<void> {
    try {
      const to = await api.backUpDatabase();
      if (to) this.toast(`Backed up to ${to.replace(/^\/Users\/[^/]+/, '~')}`);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Settings › Notifications. Turning it on asks macOS if it hasn't been asked (Rust). */
  async setNotifications(on: boolean): Promise<void> {
    await this.saveSettings({ ...this.settings, notifications: on });
  }

  /** Whether macOS lets Airboss notify, for Settings' explanation. */
  async notificationPermission(): Promise<NotificationPermission> {
    try {
      return await api.notificationPermission();
    } catch {
      return 'unknown';
    }
  }

  async openNotificationSettings(): Promise<void> {
    try {
      await api.openNotificationSettings();
    } catch (e) {
      this.fail(e);
    }
  }

  async setOpenAtLogin(on: boolean): Promise<void> {
    await this.saveSettings({ ...this.settings, open_at_login: on });
  }

  async copy(what: 'agent_note' | 'setup_command' | 'codex_setup'): Promise<void> {
    try {
      await api.copyText(what);
      this.toast(what === 'agent_note' ? 'Copied the agent note' : what === 'codex_setup' ? 'Copied the Codex setup. Paste it into ~/.codex/config.toml.' : 'Copied the setup command');
    } catch (e) {
      this.fail(e);
    }
  }

  // ================================================================== commands and keys

  /** Plans grid columns, measured by the grid. */
  planCols = 1;
  private seq = new Sequencer(['g']);
  private toastReturn: HTMLElement | null = null;

  /** The one keydown listener on window: every shortcut the page owns. */
  handleKey(e: KeyboardEvent): void {
    if (e.defaultPrevented || e.isComposing) return;
    const key0 = keyOf(e);
    if (!key0) return;
    const active = document.activeElement;
    const zone = zoneOf(active, !!this.ui.overlay);
    if (zone !== this.ui.zone && zone !== 'overlay') this.setZone(zone);
    if (this.ui.overlay) {
      // An overlay that didn't handle the key itself (focus escaped it): Esc still closes it,
      // and Tab goes back inside.
      if (key0 === 'esc') {
        e.preventDefault();
        return this.close();
      }
      if (key0 === 'tab' || key0 === 'shift+tab') {
        const roots = document.querySelectorAll<HTMLElement>('[data-zone="overlay"] [role="dialog"], [data-zone="overlay"][role="dialog"], [data-zone="overlay"]');
        const root = roots[roots.length - 1];
        if (root && !root.contains(active)) {
          e.preventDefault();
          (nextFocusable(focusables(root), null, key0 === 'shift+tab') ?? root).focus();
          return;
        }
      }
    }
    // A field: only ⌘K, ⌘N, ⌘, and F6 act; Esc leaves a field that doesn't handle it itself.
    if (zone === 'field' && key0 === 'esc') {
      e.preventDefault();
      return this.leaveField(active as HTMLElement);
    }
    const letters = zone === 'list' || zone === 'panel' || zone === 'sidebar';
    let key = key0;
    if (letters && /^[a-z]$/.test(key0)) {
      key = this.seq.feed(key0);
      if (!key) {
        e.preventDefault();
        return;
      }
    } else this.seq.reset();
    // ↩ and Space press a focused button (in the panel, or a Next up pill on a plan card).
    if ((zone === 'panel' || zone === 'list') && controlOwnsKey(active, key)) return;
    const cmd = resolve(key, zone, this.viewKind, this.actionTask() !== null || this.ui.selected !== null);
    if (!cmd) {
      // Tab moves through every control in document order, buttons included, whatever macOS's
      // Keyboard navigation setting says (WebKit skips buttons when it's off).
      if ((key === 'tab' || key === 'shift+tab') && !this.ui.overlay) {
        const root = document.getElementById('app') ?? document.body;
        const next = nextFocusable(focusables(root), active, key === 'shift+tab');
        if (next) {
          e.preventDefault();
          next.focus();
        }
      }
      return;
    }
    e.preventDefault();
    if (SETTLE_GUARDED.has(cmd.id) && this.settling()) return;
    this.runCommand(cmd.id, zone);
  }

  runCommand(id: CommandId, zone: FocusZone = this.ui.zone): void {
    const sel = this.ui.selected;
    switch (id) {
      case 'go_needs':
      case 'open_needs':
        return this.go('needs');
      case 'go_plans':
        return this.go('plans');
      case 'go_ready':
        return this.go('ready');
      case 'go_all':
        return this.go('all');
      case 'go_progress':
        return this.go('progress');
      case 'layout_tree':
        return this.setLayout('tree');
      case 'layout_status':
        return this.setLayout('status');
      case 'jump':
        return this.open({ kind: 'jump', mode: 'jump' });
      case 'new_task':
      case 'quick_add':
        return this.openNew();
      case 'settings':
        return this.open({ kind: 'settings' });
      case 'shortcuts':
        return this.open({ kind: 'sheet' });
      case 'undo':
        return void this.undo();
      case 'toasts':
        return this.focusToasts();
      case 'check_connections':
        return void this.recheck();
      case 'up':
        return this.moveSelection(-1);
      case 'down':
        return this.moveSelection(1);
      case 'first':
        return this.moveSelection('first');
      case 'last':
        return this.moveSelection('last');
      case 'page_up':
        return this.moveSelection(-Math.max(1, Math.floor((this.focusTargets.list?.clientHeight ?? 380) / 38) - 1));
      case 'page_down':
        return this.moveSelection(Math.max(1, Math.floor((this.focusTargets.list?.clientHeight ?? 380) / 38) - 1));
      case 'open_fold':
        return this.arrowRight();
      case 'close_fold':
        return this.arrowLeft();
      case 'rename':
        if (sel !== null && !this.ui.pageOpen) this.ui.renaming = sel;
        return;
      case 'to_panel':
        if (sel === null) return;
        if (!this.ui.panelOpen) this.openPanel(sel);
        return this.focus('panel');
      case 'toggle_done':
        return void this.toggleDone();
      case 'status': {
        const id2 = this.actionTask();
        if (id2 === null) return;
        return this.open({ kind: 'status', task: id2, at: this.anchor(id2) });
      }
      case 'link_repos': {
        const id2 = this.actionTask();
        if (id2 === null) return;
        return this.open({ kind: 'repos', task: id2, at: this.anchor(id2, 'row') });
      }
      case 'do_it':
        return void this.doIt();
      case 'plan_it':
        return void this.planIt();
      case 'open_page':
        return this.openPage(this.actionTask());
      case 'back':
        return this.closePage();
      case 'move_up':
        return void this.reorder('up');
      case 'move_down':
        return void this.reorder('down');
      case 'add_waits_on': {
        const id2 = this.actionTask();
        if (id2 !== null) this.open({ kind: 'jump', mode: 'waits_on', task: id2 });
        return;
      }
      case 'move_under': {
        const id2 = this.actionTask();
        if (id2 !== null) this.open({ kind: 'jump', mode: 'move_under', task: id2 });
        return;
      }
      case 'add_link': {
        const id2 = this.actionTask();
        if (id2 !== null) this.open({ kind: 'link', task: id2, at: this.anchor(id2, this.ui.panelOpen || this.ui.pageOpen ? 'do' : 'row') });
        return;
      }
      case 'mark_blocked': {
        const id2 = this.actionTask();
        if (id2 !== null) void this.setStatus(id2, 'blocked');
        return;
      }
      case 'open_latest':
        return void this.openLatest(this.actionTask());
      case 'drop': {
        const id2 = this.actionTask();
        if (id2 !== null) void this.setStatus(id2, 'dropped');
        return;
      }
      case 'copy_number':
        return void this.copyNumber(this.actionTask());
      case 'escape':
        if (zone === 'sidebar' || (zone === 'panel' && !this.ui.pageOpen)) return this.focus('list');
        return this.escape();
      case 'needs_up':
        return this.moveNeed(-1);
      case 'needs_down':
        return this.moveNeed(1);
      case 'needs_main':
        return void this.mainAction();
      case 'needs_dismiss':
        return void this.dismiss();
      case 'needs_done':
        return void this.markNeedDone();
      case 'plans_left':
        return this.movePlan(-1);
      case 'plans_right':
        return this.movePlan(1);
      case 'plans_up':
        return this.movePlan(-this.planCols);
      case 'plans_down':
        return this.movePlan(this.planCols);
      case 'plans_open': {
        const card = this.plans[this.ui.planIndex];
        if (card) this.openPage(card.id);
        return;
      }
    }
  }

  movePlan(delta: number): void {
    if (!this.plans.length) return;
    this.ui.planIndex = Math.max(0, Math.min(this.plans.length - 1, this.ui.planIndex + delta));
    this.ui.keyboard = true;
  }

  /** F6: to the newest toast, and back. */
  focusToasts(): void {
    const toasts = document.querySelectorAll<HTMLElement>('[data-toast]');
    const newest = toasts[toasts.length - 1];
    if (newest && newest.contains(document.activeElement)) {
      const back = this.toastReturn;
      this.toastReturn = null;
      if (back?.isConnected) back.focus();
      else this.focus('list');
      return;
    }
    if (!newest) return;
    this.toastReturn = document.activeElement as HTMLElement | null;
    (newest.querySelector<HTMLElement>('button') ?? newest).focus();
  }

  // ================================================================== focus context

  setZone(zone: FocusZone): void {
    this.ui.zone = zone;
    this.sendContext();
  }

  sendContext(): void {
    if (this.contextFrame) return;
    this.contextFrame = requestAnimationFrame(() => {
      this.contextFrame = 0;
      void api
        .setFocusContext({ window: 'main', zone: this.ui.overlay ? 'overlay' : this.ui.zone, selection: this.actionTask(), view: this.ui.view })
        .catch(() => {});
    });
  }

  row(id: TaskId | null): RowDto | undefined {
    return this.tasks.get(id);
  }
}

function parseViewKind(view: View): { kind: string; id: number | null } {
  const m = view.match(/^(repo|parent):(\d+)$/);
  return m ? { kind: m[1]!, id: Number(m[2]) } : { kind: view, id: null };
}

function camel(command: string): string {
  return command.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
}
