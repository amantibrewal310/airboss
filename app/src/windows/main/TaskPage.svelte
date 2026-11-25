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
  import Threads from './detail/Threads.svelte';
  import Activity from './detail/Activity.svelte';
  import Subtasks from './detail/Subtasks.svelte';
  import Skeleton from './detail/Skeleton.svelte';

  /** The full task page: a centred document column (max 860px) and a 280px properties rail. */
  const app = useApp();
  const row = $derived(app.row(app.ui.selected));
  const detail = $derived(app.detail.detail);
  const fresh = $derived(detail !== null && row !== undefined && detail.task.id === row.id);
  const crumbs = $derived(row ? ancestorsOf(app.tasks.rows, row.id).reverse() : []);
  const map = $derived(app.runMap ?? (fresh ? detail!.waves : null));
  let columnWidth = $state(860);
</script>

{#if row}
  <div class="pagewrap" data-zone="panel" bind:this={app.focusTargets.page} tabindex="-1" aria-label="Task {row.id}">
    <div class="head" data-tauri-drag-region>
      <nav class="crumb" aria-label="Parents">
        {#each crumbs as c (c)}
          <button type="button" onclick={() => app.openPage(c)}>{app.row(c)?.title}</button><span aria-hidden="true">›</span>
        {/each}
        <b>{row.id}</b>
      </nav>
      <span class="sp" data-tauri-drag-region></span>
      <DoItButtons task={row.id} />
      <button type="button" class="ib" title="Back (⌘↑ or Esc)" aria-label="Back to the list" onclick={() => app.closePage()}><Icon name="close" size={14} /></button>
    </div>
    {#if !app.conn.t3Ready}<T3Banner />{/if}
    <div class="page">
      <div class="content">
        <article class="doc" bind:clientWidth={columnWidth}>
          <Title task={row.id} title={row.title} size="page" />
          {#if app.detail.skeleton && !fresh}
            <div class="gap"><Skeleton lines={8} /></div>
          {:else}
            {@const items = app.needs.forTask(row.id)}
            {#if items.length}<div class="gap calls"><Callouts {items} /></div>{/if}
            <div class="notes"><Notes task={row.id} body={fresh ? detail!.task.body : null} empty="No notes yet." /></div>
            {#if fresh && detail!.links.some((l) => l.thread)}<div class="threads"><Threads links={detail!.links} /></div>{/if}
            {#if map && map.nodes.length && row.progress}
              <Subtasks {map} progress={row.progress} width={columnWidth - 80} />
            {/if}
            {#if fresh}
              <div class="activity"><Activity task={row.id} history={detail!.history} more={detail!.history_more} /></div>
            {/if}
          {/if}
        </article>
      </div>
      <aside class="rail" aria-label="Properties">
        <Props {row} {detail} rail />
      </aside>
    </div>
  </div>
{/if}

<style>
  .pagewrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    outline: none;
  }
  .head {
    height: var(--header-h);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px 0 20px;
    border-bottom: 1px solid var(--line);
    flex: none;
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
  }
  .ib:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .page {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    display: flex;
    justify-content: center;
  }
  .doc {
    width: 100%;
    max-width: 860px;
    padding: 30px 40px 60px;
    display: flex;
    flex-direction: column;
  }
  .gap {
    padding-top: 16px;
  }
  .calls {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .notes {
    padding-top: 14px;
  }
  .threads {
    padding-top: 20px;
  }
  .activity {
    padding-top: 20px;
  }
  .rail {
    width: 280px;
    flex: none;
    border-left: 1px solid var(--line);
    padding: 16px 18px;
    overflow-y: auto;
    background: var(--bg);
  }
</style>
