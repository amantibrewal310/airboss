<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api, on } from '../../lib/api';
  import type { CreateTaskInput, DoItPlace, QuickaddData, Repo, TaskId } from '../../lib/types';
  import NewTask from '../shared/NewTask.svelte';
  import StartInMenu from '../main/overlays/StartInMenu.svelte';

  /**
   * The quick-add panel: the whole window is New task. It is reset ahead of time (on every hide),
   * so showing it does no I/O and no rendering. A hide caused by clicking elsewhere keeps a
   * non-empty title and notes; Esc and Create always reset.
   */
  let { native }: { native: boolean } = $props();
  let data = $state<QuickaddData>({ repos: [], parents: [] });
  let hotkey = $state('Control+Alt+N');
  let form: NewTask | undefined = $state();
  let wrap: HTMLDivElement | undefined = $state();
  let startIn = $state<{ task: TaskId; choices: Repo[]; x: number; y: number } | null>(null);
  let cause: 'esc' | 'create' | null = null;
  let busy = $state(false);

  async function refresh() {
    try {
      data = await api.quickaddData();
    } catch {
      // Keep the last data; the panel still works for a title.
    }
  }

  async function resetForm() {
    form?.reset(cause === null);
    cause = null;
    startIn = null;
    await refresh();
    await tick();
    form?.focusTitle();
  }

  function hide(why: 'esc' | 'create') {
    cause = why;
    void api.quickaddHide().catch(() => {});
  }

  async function submit(input: CreateTaskInput) {
    if (busy) return;
    busy = true;
    try {
      const res = await api.createTask(input);
      if (res.launch && 'needs_choice' in res.launch) {
        // The Start in menu opens in the panel; the panel stays until a choice is made.
        const btn = wrap?.querySelector('.go')?.getBoundingClientRect();
        startIn = { task: res.id, choices: res.launch.needs_choice.choices, x: btn ? btn.left - 100 : 300, y: btn ? btn.bottom + 6 : 200 };
        return;
      }
      hide('create');
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
    hide('create');
    await api.doIt(s.task, place, false).catch(() => {});
  }

  onMount(() => {
    const unsub = [on('quickadd:reset', (r) => ((hotkey = r.hotkey), void resetForm()))];
    void refresh().then(() => form?.focusTitle());
    // Height follows the content (a dropdown included).
    const ro = new ResizeObserver(() => {
      if (!wrap) return;
      const dd = wrap.querySelector('.dd, [role="menu"]')?.getBoundingClientRect();
      const h = Math.ceil(Math.max(wrap.getBoundingClientRect().bottom, dd ? dd.bottom + 8 : 0));
      void api.quickaddResize(h).catch(() => {});
    });
    if (wrap) ro.observe(wrap);
    const onFocus = () => void api.quickaddShown().catch(() => {});
    window.addEventListener('focus', onFocus);
    void api.uiReady('quickadd').catch(() => {});
    return () => {
      ro.disconnect();
      window.removeEventListener('focus', onFocus);
      void Promise.all(unsub).then((fns) => fns.forEach((f) => f()));
    };
  });
</script>

<div class="stage" class:native>
  <div class="card" bind:this={wrap}>
    <NewTask
      bind:this={form}
      repos={data.repos}
      parents={data.parents}
      {hotkey}
      inApp={false}
      onsubmit={submit}
      oncancel={() => hide('esc')}
    />
  </div>
  {#if startIn}
    <StartInMenu task={startIn.task} x={startIn.x} y={startIn.y} choices={startIn.choices} onpick={pick} onclose={() => hide('esc')} />
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
  .native .card {
    box-shadow: none;
    border: 1px solid var(--line-2);
  }
</style>
