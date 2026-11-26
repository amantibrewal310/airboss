<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, on } from '../../lib/api';
  import type { CreateTaskInput, DoItPlace, QuickaddData, Repo, TaskId } from '../../lib/types';
  import type { Component } from 'svelte';
  import NewTask from '../shared/NewTask.svelte';

  /**
   * The quick-add panel: the whole window is New task. It is reset ahead of time (on every hide),
   * so showing it does no I/O and no rendering. Rust says whether to keep the text: a hide caused
   * by clicking elsewhere (`keep_text`) keeps the title and notes and only refocuses; Esc and
   * Create reset everything.
   */
  let { native }: { native: boolean } = $props();
  let data = $state<QuickaddData>({ repos: [], parents: [] });
  let hotkey = $state('Control+Alt+N');
  let form: NewTask | undefined = $state();
  let wrap: HTMLDivElement | undefined = $state();
  let startIn = $state<{ task: TaskId; choices: Repo[]; x: number; y: number } | null>(null);
  let busy = $state(false);
  /** The Start in menu is loaded only when a Do it needs a repo choice, keeping the panel small. */
  let StartInMenu = $state<Component<{ task: TaskId; x: number; y: number; choices: Repo[]; onpick: (p: DoItPlace) => void; onclose: () => void }> | null>(null);
  const loadStartIn = () => import('../main/overlays/StartInMenu.svelte').then((m) => (StartInMenu = m.default as never));

  async function refresh() {
    try {
      data = await api.quickaddData();
    } catch {
      // Keep the last data; the panel still works for a title.
    }
  }

  async function resetForm(keepText: boolean) {
    if (keepText) {
      await tick();
      form?.focusTitle();
      return;
    }
    form?.reset(false);
    startIn = null;
    await refresh();
    await tick();
    form?.focusTitle();
  }

  function hide() {
    void api.quickaddHide().catch(() => {});
  }

  async function submit(input: CreateTaskInput) {
    if (busy) return;
    busy = true;
    try {
      const res = await api.createTask(input);
      if (res.launch && 'needs_choice' in res.launch) {
        if (!StartInMenu) await loadStartIn();
        // The Start in menu opens in the panel; the panel stays until a choice is made.
        const btn = wrap?.querySelector('.go')?.getBoundingClientRect();
        startIn = { task: res.id, choices: res.launch.needs_choice.choices, x: btn ? btn.left - 100 : 300, y: btn ? btn.bottom + 6 : 200 };
        return;
      }
      hide();
    } catch {
      // Rust shows the main window with the error for anything after the task exists; an empty
      // title can't be submitted.
    } finally {
      busy = false;
    }
  }

  async function pick(place: DoItPlace) {
    const s = startIn;
    startIn = null;
    if (!s) return;
    hide();
    await api.doIt(s.task, place, false).catch(() => {});
  }

  /** What hangs below the card: a picker (Repos, Parent) or the Start in menu. */
  const POPUPS = '.dd, [role="menu"]';
  /** Room below a popup for its shadow. */
  const POPUP_MARGIN = 24;
  let stage: HTMLDivElement | undefined = $state();
  let sent = 0;
  let settleFrame = 0;

  /** The window's height: the card, or down past an open popup so all of it shows. */
  function measure(): number {
    if (!wrap || !stage) return 0;
    let bottom = wrap.getBoundingClientRect().bottom;
    for (const el of stage.querySelectorAll(POPUPS)) bottom = Math.max(bottom, el.getBoundingClientRect().bottom + POPUP_MARGIN);
    return Math.ceil(bottom);
  }

  /** Sizes the window to the content; Rust keeps its top edge where it is. */
  function fit() {
    const h = measure();
    if (!h || h === sent) return;
    sent = h;
    void api.quickaddResize(h).catch(() => {});
    settle();
  }

  /**
   * Once the new content has painted, sizes the window again (the same height) so Rust redraws
   * its shadow around what is now on screen.
   */
  function settle() {
    cancelAnimationFrame(settleFrame);
    settleFrame = requestAnimationFrame(() => {
      settleFrame = requestAnimationFrame(() => {
        settleFrame = 0;
        const h = measure();
        if (!h) return;
        sent = h;
        void api.quickaddResize(h).catch(() => {});
      });
    });
  }

  onMount(() => {
    const unsub = [on('quickadd:reset', (r) => ((hotkey = r.hotkey), void resetForm(r.keep_text === true)))];
    void refresh().then(() => form?.focusTitle());
    // The height follows the card, and grows while a picker hangs below it. A picker is
    // positioned absolutely, so the card's own size doesn't change: watch for it coming and going.
    const ro = new ResizeObserver(fit);
    const watch = () => {
      ro.disconnect();
      if (wrap) ro.observe(wrap);
      stage?.querySelectorAll(POPUPS).forEach((el) => ro.observe(el));
      fit();
    };
    const mo = new MutationObserver(watch);
    if (stage) mo.observe(stage, { childList: true, subtree: true });
    watch();
    const onFocus = () => {
      settle();
      void api.quickaddShown().catch(() => {});
      // Repos and tasks may have changed in the main window since the panel last hid.
      void refresh();
    };
    window.addEventListener('focus', onFocus);
    void api.uiReady('quickadd').catch(() => {});
    return () => {
      ro.disconnect();
      mo.disconnect();
      cancelAnimationFrame(settleFrame);
      window.removeEventListener('focus', onFocus);
      void Promise.all(unsub).then((fns) => fns.forEach((f) => f()));
    };
  });
</script>

<div class="stage" class:native bind:this={stage} onanimationend={settle}>
  <div class="card" bind:this={wrap}>
    <NewTask
      bind:this={form}
      repos={data.repos}
      parents={data.parents}
      {hotkey}
      inApp={false}
      onsubmit={submit}
      oncancel={() => hide()}
    />
  </div>
  {#if startIn && StartInMenu}
    <StartInMenu task={startIn.task} x={startIn.x} y={startIn.y} choices={startIn.choices} onpick={pick} onclose={() => hide()} />
  {/if}
</div>

<style>
  :global(body.quickadd) {
    background: transparent;
  }
  .stage {
    padding: 0;
  }
  .stage:not(.native) {
    min-height: 100vh;
    background: var(--desk);
    display: flex;
    justify-content: center;
    padding-top: 16vh;
  }
  .card {
    width: 680px;
    border-radius: var(--r-xl);
    background: var(--bg-raised);
    box-shadow: var(--shadow-float);
    align-self: flex-start;
  }
  /* In the app, AppKit draws the panel's shadow around the card (a CSS one would need a
     transparent margin around it, taking clicks meant for the app behind). */
  .native .card {
    box-shadow: none;
    border: 1px solid var(--line-2);
  }
  /* A picker's shadow fits in the room left below it. */
  .native :global(.dd) {
    box-shadow:
      0 6px 18px rgb(0 0 0 / 0.22),
      0 0 0 1px var(--line-2);
  }
</style>
