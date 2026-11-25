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
  RepoId,
  RowDto,
  RunMap,
  Settings,
  Status,
  TaskId,
  ThreadKey,
  ToastEvent,
  UndoableWrite,
} from '../types';
import { now as clockNow } from '../logic/clock';
import type { CommandId } from '../logic/commands';
import { controlOwnsKey, zoneOf } from '../logic/focus';
import { keyOf, Sequencer } from '../logic/keymap';
import { resolve } from '../logic/commands';
import { applyAccent, applyAppearance } from '../logic/theme';
import { launchToast } from '../logic/launch';
import { keepMapSelection, keepSelection, reveal as revealPlan, selectionAfterFold } from '../logic/selection';
import {
  isListView,
  isTreeLayout,
  isView,
  parentsList,
  sidebarCounts,
  SHAPE_LABEL,
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

const DEFAULT_SETTINGS: Settings = { hotkey: 'Control+Alt+N', open_at_login: true, appearance: 'system', folded: [] };

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
  now = $state(clockNow());
  plans = $state.raw<PlanCard[]>([]);
  plansLoaded = $state(false);
  runMap = $state.raw<RunMap | null>(null);
  /** Native app (Tauri): real traffic lights and drag regions. */
  native = false;

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
      this.conn.set(b.connections, b.availability);
      this.settings = b.settings;
      this.cli = b.cli;
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
      on('connections:changed', (c) => this.conn.set(c.connections, c.availability)),
      on('menu:action', ({ command }) => this.runCommand(command as CommandId)),
      on('nav:open', (n) => this.navOpen(n)),
      on('toast', (t) => this.toastEvent(t)),
      on('appearance:changed', (a) => applyAccent(document.documentElement, a.accent)),
      on('error:background', (e) => (this.conn.background = e.message)),
    ]);
    const tick = () => (this.now = clockNow());
    setInterval(() => {
      if (document.visibilityState === 'visible') tick();
    }, 60_000);
    document.addEventListener('visibilitychange', tick);
    this.ready = true;
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
      this.toast(`Task ${selected} no longer exists.`, 'warn');
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
    if (!this.ui.overlay) this.ui.returnFocus = document.activeElement as HTMLElement | null;
    this.ui.overlay = o;
  }

  close(): void {
    this.ui.overlay = null;
    const el = this.ui.returnFocus;
    this.ui.returnFocus = null;
    queueMicrotask(() => (el && el.isConnected ? el.focus({ preventScroll: true }) : this.focus('list')));
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

  fail(e: unknown): void {
    const err = asAppError(e);
    if (err.code === 'already_starting') return this.toast('Already starting — check T3 in a moment.');
    this.toast(err.message, 'warn');
  }

  private undoToast(text: string, write: UndoableWrite, local?: { key?: string; rows?: TaskId[] }): void {
    const undo = write.undo;
    if (undo && local) this.undoLocal.set(undo.id, local);
    this.toasts.push({
      text,
      tone: 'info',
      actions: undo ? [{ label: 'Undo', run: () => this.undo(undo.id) }] : [],
      ...(undo ? { undo } : {}),
    });
  }

  private toastEvent(t: ToastEvent): void {
    this.toasts.push({
      text: t.text,
      tone: 'info',
      actions: [
        ...t.actions.map((a) => ({
          label: a.label,
          run: () => void (api as unknown as Record<string, (...x: unknown[]) => Promise<unknown>>)[camel(a.command)]?.(...Object.values(a.args)),
        })),
        ...(t.undo ? [{ label: 'Undo', run: () => this.undo(t.undo!.id) }] : []),
      ],
    });
  }

  // ================================================================== writes

  private applyWrite(p: Patch | null | undefined): void {
    if (p) this.onPatch(p);
  }

  async toggleDone(id: TaskId | null = this.ui.selected): Promise<void> {
    const r = this.tasks.get(id);
    if (!r || r.status === 'proposed') return;
    try {
      const w = await api.toggleDone(r.id);
      this.applyWrite(w.patch);
      const now = this.tasks.get(r.id);
      this.undoToast(now?.status === 'done' ? `${r.id} marked done` : `${r.id} marked not done`, w);
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
      if (w.patch) this.undoToast(`${id} → ${SHAPE_LABEL[status]}`, w);
    } catch (e) {
      this.fail(e);
    }
  }

  async drop(id: TaskId): Promise<void> {
    try {
      const w = await api.setStatus(id, 'dropped', null);
      this.applyWrite(w.patch);
      this.undoToast(`Dropped ${id}`, w);
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
      this.toast(`${id} now waits on ${on}`);
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
      this.undoToast(parent === null ? `Moved ${id} to the top level` : `Moved ${id} under ${parent}`, w);
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
      if (w.patch) this.undoToast(`Moved ${id} ${direction}`, w);
    } catch (e) {
      this.fail(e);
    }
  }

  async copyNumber(id: TaskId | null = this.ui.selected): Promise<void> {
    if (id === null) return;
    try {
      await api.copyText({ task_number: id });
      this.toast(`Copied ${id}`);
    } catch (e) {
      this.fail(e);
    }
  }

  openLink(l: LinkDto): void {
    const p = l.thread ? api.openThread(l.thread.key) : api.openUrl({ link: l.id });
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
      .filter((l) => (l.thread && (l.thread.url || l.kind === 't3_thread')) || (l.kind === 'pr' && (l.url || l.pr?.url)))
      .sort((a, b) => b.linked_at.localeCompare(a.linked_at));
    const latest = candidates[0];
    if (!latest) return this.toast(`${id} has no thread or PR yet`);
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
    if (!this.conn.t3Ready) {
      return this.toast(this.conn.t3Missing ?? 'Do it and Plan it need T3 Code. Install it, then choose Check again.', 'warn');
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

  /** Hides the item at once (180 ms exit), then the selection moves to the item at the same index. */
  private leave(item: NeedsItem, ms?: number): void {
    const before = this.needs.visible.map((n) => n.key);
    this.needs.hideLocally(item.key, ms, () => this.reconcileNeeds(before));
  }

  async mainAction(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item) return;
    try {
      switch (item.kind) {
        case 'question':
        case 'approval':
        case 'failed_run':
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
          this.undoToast(`${item.task} marked done`, w);
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

  async dismiss(item: NeedsItem | null = this.needsItem, text = 'Dismissed'): Promise<void> {
    if (!item) return;
    this.leave(item);
    try {
      const w = await api.needsDismiss(item.key);
      this.undoToast(text, w, { key: item.key });
    } catch (e) {
      this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  async snooze(item: NeedsItem | null = this.needsItem): Promise<void> {
    if (!item) return;
    this.leave(item);
    try {
      const w = await api.needsSnooze(item.key, null);
      this.undoToast('Snoozed for 3 days', w, { key: item.key });
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
      this.toast(ids.length === 1 ? `Accepted ${ids[0]}` : `Added ${ids.length} subtasks to ${item.task}`);
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
    this.tasks.hide(ids, true);
    if (all) this.leave(item, 7000);
    try {
      const w = await api.rejectProposals(ids, null);
      this.undoToast(ids.length === 1 ? `Rejected ${ids[0]}` : `Rejected ${ids.length} subtasks`, w, {
        rows: ids,
        ...(all ? { key: item.key } : {}),
      });
    } catch (e) {
      this.tasks.hide(ids, false);
      if (all) this.needs.unhide(item.key);
      this.fail(e);
    }
  }

  // ================================================================== undo

  async undo(entry: number | null = null): Promise<void> {
    try {
      const res = await api.undo(entry);
      this.applyWrite(res.patch);
      const local = entry !== null ? this.undoLocal.get(entry) : [...this.undoLocal.entries()].pop()?.[1];
      if (entry !== null) this.undoLocal.delete(entry);
      if (local?.key) this.needs.unhide(local.key);
      if (local?.rows) this.tasks.hide(local.rows, false);
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
      for (const l of res.link_errors) this.toast(`Created ${res.id}, but couldn't link ${l.text}`, 'warn');
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
      this.toast(r.on_path ? `Installed airboss at ${r.path}` : `Installed at ${r.path}. Add ~/.local/bin to your PATH.`);
    } catch (e) {
      this.fail(e);
    }
  }

  async copy(what: 'agent_note' | 'setup_command'): Promise<void> {
    try {
      await api.copyText(what);
      this.toast(what === 'agent_note' ? 'Copied the agent note' : 'Copied the setup command');
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
    // A field: only ⌘K, ⌘N, ⌘, and F6 act; Esc leaves a field that doesn't handle it itself.
    if (zone === 'field' && key0 === 'esc') {
      (active as HTMLElement).blur();
      e.preventDefault();
      return;
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
    if (zone === 'panel' && controlOwnsKey(active, key)) return;
    const cmd = resolve(key, zone, this.viewKind, this.actionTask() !== null || this.ui.selected !== null);
    if (!cmd) return;
    e.preventDefault();
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
        if (sel !== null) this.ui.renaming = sel;
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
        return this.openPage();
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
