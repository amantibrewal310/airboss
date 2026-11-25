<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { App } from '../../lib/state/app.svelte';
  import { provideApp } from '../../lib/state/context';
  import { zoneOf } from '../../lib/logic/focus';
  import Sidebar from './Sidebar.svelte';
  import Header from './Header.svelte';
  import TaskList from './TaskList.svelte';

  let { app }: { app: App } = $props();
  // svelte-ignore state_referenced_locally
  provideApp(app);

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

<svelte:window onkeydown={(e) => app.handleKey(e)} />
<svelte:document onfocusin={() => app.setZone(zoneOf(document.activeElement, !!app.ui.overlay))} />

<div class="win">
  <Sidebar />
  <main class="main">
    <Header />
    <div class="body">
      <TaskList />
    </div>
  </main>
</div>

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
