<script lang="ts">
  import Segmented from '../../../lib/components/Segmented.svelte';
  import StackedBar from '../../../lib/components/StackedBar.svelte';
  import type { Progress, RunMap as RunMapDto } from '../../../lib/types';
  import { mapFits } from '../../../lib/logic/runMapLayout';
  import RunMap from './RunMap.svelte';
  import WavesList from './WavesList.svelte';

  /**
   * The page's Subtasks section: "N waves", the 8px stacked bar with its legend, the Waves / Map
   * switch (opening on Map unless the map is too big), and the reminder that nothing starts by itself.
   */
  let { map, progress, width }: { map: RunMapDto; progress: Progress; width: number } = $props();
  const fits = $derived(mapFits(map));
  let mode = $state<'map' | 'list'>('map');
  const showing = $derived(fits ? mode : 'list');
  const pct = $derived(progress.total ? Math.round((progress.done / progress.total) * 100) : 0);
  const todo = $derived(progress.total - progress.done - progress.in_review - progress.doing - progress.blocked);
  const legend = $derived(
    [
      ['Done', progress.done, 'var(--st-done)'],
      ['In review', progress.in_review, 'var(--st-review)'],
      ['In progress', progress.doing, 'var(--st-doing)'],
      ['Blocked', progress.blocked, 'var(--st-blocked)'],
    ].filter(([, n]) => (n as number) > 0) as [string, number, string][],
  );
</script>

<section class="sec" aria-label="Subtasks">
  <div class="h">
    <h3>Subtasks</h3>
    <span class="n">{map.waves.length} {map.waves.length === 1 ? 'wave' : 'waves'}</span>
    <span class="sp"></span>
    {#if fits}
      <Segmented
        label="Show subtasks as"
        options={[
          { value: 'list', label: 'Waves' },
          { value: 'map', label: 'Map' },
        ] as { value: 'map' | 'list'; label: string }[]}
        value={mode}
        onchange={(v) => (mode = v)}
      />
    {:else}
      <span class="n" title="The map is drawn for up to 40 subtasks and 8 waves">Map shows up to 40 subtasks</span>
    {/if}
  </div>
  <div class="big">
    <span class="pct">{pct}%</span>
    <StackedBar p={progress} height={8} />
    <span class="of">{progress.done} of {progress.total} done</span>
  </div>
  <div class="legend">
    {#each legend as [label, n, color] (label)}<span><i style:background={color}></i>{label} {n}</span>{/each}
    {#if todo > 0}<span><i class="todo"></i>Todo {todo}</span>{/if}
  </div>
  {#if showing === 'map'}
    <RunMap {map} {width} />
  {:else}
    <WavesList {map} />
  {/if}
  <p class="note">Nothing starts by itself. Select a ready task and press ⌘↩.</p>
</section>

<style>
  .sec {
    display: flex;
    flex-direction: column;
    padding-top: 26px;
  }
  .h {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-bottom: 10px;
  }
  h3 {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
  }
  .n {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--text-3);
  }
  .sp {
    flex: 1;
  }
  .big {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 4px 0 8px;
  }
  .pct {
    font-size: var(--fs-progress);
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 28px;
    min-width: 58px;
    font-variant-numeric: tabular-nums;
  }
  .of {
    color: var(--text-3);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    padding-bottom: 18px;
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-2);
  }
  .legend span {
    display: flex;
    align-items: center;
    gap: 6px;
    font-variant-numeric: tabular-nums;
  }
  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }
  .legend .todo {
    background: var(--bg-input);
    box-shadow: inset 0 0 0 1px var(--line-2);
  }
  .note {
    padding-top: 10px;
    font-size: var(--fs-meta);
    color: var(--text-3);
  }
</style>
