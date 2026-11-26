<script lang="ts">
  import Icon from '../../lib/components/Icon.svelte';
  import { useApp } from '../../lib/state/context';
  import { ancestorsOf } from '../../lib/logic/views';
  import DoItButtons from './detail/DoItButtons.svelte';
  import T3Banner from './detail/T3Banner.svelte';
  import Title from './detail/Title.svelte';
  import Callouts from './detail/Callouts.svelte';
  import Props from './detail/Props.svelte';
  import Notes from './detail/Notes.svelte';
  import WavesList from './detail/WavesList.svelte';
  import Threads from './detail/Threads.svelte';
  import Activity from './detail/Activity.svelte';
  import Skeleton from './detail/Skeleton.svelte';

  /**
   * The 480px panel (380px when the window is 980px or less). The header and title come from the
   * row at once; the sections wait for `task_detail`, keeping the previous task until it arrives.
   */
  const app = useApp();
  const id = $derived(app.ui.selected);
  const row = $derived(app.row(id));
  const detail = $derived(app.detail.detail);
  const fresh = $derived(detail !== null && detail.task.id === id);
  const crumbs = $derived(row ? ancestorsOf(app.tasks.rows, row.id).reverse() : []);
</script>

{#if row}
  <aside class="peek" data-zone="panel" bind:this={app.focusTargets.panel} tabindex="-1" aria-label="Task {row.id}">
    <div class="head" data-tauri-drag-region>
      <nav class="crumb" aria-label="Parents">
        {#each crumbs as c (c)}
          <button type="button" onclick={() => app.select(c, { open: true })}>{app.row(c)?.title}</button><span aria-hidden="true">›</span>
        {/each}
        <b>{row.id}</b>
      </nav>
      <span class="sp" data-tauri-drag-region></span>
      <DoItButtons task={row.id} />
      <button type="button" class="ib" title="Open page (⌘↓)" aria-label="Open page" onclick={() => app.openPage(row.id)}><Icon name="expand" size={14} /></button>
      <button type="button" class="ib" title="Close (Esc)" aria-label="Close" onclick={() => app.closePanel()}><Icon name="close" size={14} /></button>
    </div>
    {#if !app.conn.launchReady}<T3Banner />{/if}
    <div class="pk">
      <Title task={row.id} title={row.title} />
      {#if app.detail.skeleton && !fresh}
        <Skeleton />
      {:else if app.detail.error && !fresh}
        <p class="missing">{app.detail.error}</p>
      {:else}
        <Callouts items={app.needs.forTask(row.id)} />
        <Props {row} {detail} />
        <Notes task={row.id} body={fresh ? detail!.task.body : null} />
        {#if fresh && detail!.waves && detail!.waves.nodes.length}
          <section class="sec" aria-label="Subtasks">
            <h3>Subtasks <span>{detail!.waves.waves.length} {detail!.waves.waves.length === 1 ? 'wave' : 'waves'}</span></h3>
            <WavesList map={detail!.waves} />
          </section>
        {/if}
        {#if fresh}
          <Threads links={detail!.links} />
          <Activity task={row.id} history={detail!.history} more={detail!.history_more} links={detail!.links} />
        {/if}
      {/if}
    </div>
  </aside>
{/if}

<style>
  .peek {
    width: var(--panel-w);
    flex: none;
    border-left: 1px solid var(--line);
    background: var(--bg);
    display: flex;
    flex-direction: column;
    min-height: 0;
    outline: none;
    animation: peekin var(--dur-panel) var(--ease);
  }
  /* Up to about 1180px of window the panel takes 380px, so the list keeps 480px or more. */
  @media (max-width: 1180px) {
    .peek {
      width: 380px;
    }
  }
  @keyframes peekin {
    from {
      opacity: 0;
      transform: translateX(10px);
    }
  }
  .head {
    height: var(--header-h);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px 0 20px;
    border-bottom: 1px solid var(--line);
    flex: none;
    min-width: 0;
  }
  .crumb {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--text-3);
    font-size: var(--fs-meta);
    line-height: 16px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .crumb button {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    max-width: 160px;
  }
  .crumb button:hover {
    color: var(--text-2);
  }
  .crumb b {
    color: var(--text-2);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
  .sp {
    flex: 1;
    align-self: stretch;
  }
  .ib {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    color: var(--text-2);
    border-radius: var(--r-md);
    flex: none;
  }
  .ib:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .pk {
    flex: 1;
    overflow-y: auto;
    scrollbar-width: thin;
    padding: 20px 24px 40px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .sec {
    padding-top: 6px;
  }
  .sec h3 {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
  }
  .sec h3 span {
    color: var(--text-3);
    font-weight: 500;
    margin-left: 4px;
  }
  .missing {
    color: var(--text-2);
  }
</style>
