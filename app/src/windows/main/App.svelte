<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { App } from '../../lib/state/app.svelte';
  import { provideApp } from '../../lib/state/context';
  import { zoneOf } from '../../lib/logic/focus';
  import Sidebar from './Sidebar.svelte';
  import Header from './Header.svelte';
  import TaskList from './TaskList.svelte';
  import TaskPanel from './TaskPanel.svelte';
  import TaskPage from './TaskPage.svelte';
  import NeedsInbox from './NeedsInbox.svelte';
  import PlansGrid from './PlansGrid.svelte';
  import Overlays from './Overlays.svelte';
  import Toasts from './Toasts.svelte';
  import FirstRun from './FirstRun.svelte';
  import DbError from './DbError.svelte';

  let { app }: { app: App } = $props();
  // svelte-ignore state_referenced_locally
  provideApp(app);

  // The native menus follow focus: zone, selection and view, sent at most once a frame.
  $effect(() => {
    void app.ui.zone;
    void app.ui.overlay;
    void app.ui.view;
    void app.ui.selected;
    void app.ui.mapSel;
    void app.needs.selectedKey;
    if (app.ready) app.sendContext();
  });

  /**
   * Right-click shows the app's own menus (rows, stations) and nothing elsewhere: no browser
   * menu in a Mac app. Text fields and selected text keep the system menu (Cut, Copy, Paste).
   */
  function nativeMenu(e: MouseEvent) {
    if (e.defaultPrevented) return;
    const t = e.target as Element | null;
    if (t?.closest('input, textarea, [contenteditable="true"]')) return;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.toString().trim()) return;
    e.preventDefault();
  }

  onMount(() => {
    // Rust shows the window once the bootstrap data has rendered (2 s backstop in Rust).
    const stop = $effect.root(() => {
      $effect(() => {
        if (app.ready) void api.uiReady('main').catch(() => {});
      });
    });
    return stop;
  });
</script>

<svelte:window onkeydown={(e) => app.handleKey(e)} oncontextmenu={nativeMenu} />
<svelte:document onfocusin={() => app.setZone(zoneOf(document.activeElement, !!app.ui.overlay))} />

{#if app.fatal}
  <DbError error={app.fatal} />
{:else}
<div class="win">
  <Sidebar />
  <main class="main">
    {#if app.viewKind === 'page'}
      <TaskPage />
    {:else}
      <Header />
      <div class="body">
        {#if app.firstRun && app.ui.view === 'all'}
          <FirstRun />
        {:else if app.viewKind === 'list'}
          <TaskList />
          {#if app.ui.panelOpen && app.ui.selected !== null}<TaskPanel />{/if}
        {:else if app.viewKind === 'needs'}
          <NeedsInbox />
        {:else}
          <PlansGrid />
        {/if}
      </div>
    {/if}
  </main>
</div>
<Overlays />
<Toasts />
{/if}

<style>
  .win {
    height: 100%;
    display: flex;
    background: var(--bg);
    overflow: hidden;
  }
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
</style>
