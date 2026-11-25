<script lang="ts">
  import StatusIcon from '../../../lib/components/StatusIcon.svelte';
  import RepoDot from '../../../lib/components/RepoDot.svelte';
  import Icon from '../../../lib/components/Icon.svelte';
  import type { RunMap } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { shapeOf } from '../../../lib/logic/views';

  /** The Waves list: "Wave 1 · can start now", "· in parallel", then 32px rows. */
  let { map, onopen }: { map: RunMap; onopen?: (id: number) => void } = $props();
  const app = useApp();
  const outside = $derived(new Map(map.outside.map((o) => [o.task, o.blockers])));
</script>

<div class="waves">
  {#each map.waves as ids, w (w)}
    {#if ids.length}
      <div class="wv">
        <div class="wv-h">Wave {w + 1}{#if w === 0}<span>{' · can start now'}</span>{/if}{#if ids.length > 1}<span>{' · in parallel'}</span>{/if}</div>
        {#each ids as id (id)}
          {@const r = app.row(id)}
          {#if r}
            {@const out = outside.get(id)}
            <button type="button" class="sub" class:wait={r.waiting || !!out} onclick={() => (onopen ? onopen(id) : app.reveal(id))} ondblclick={() => app.openPage(id)}>
              <StatusIcon shape={shapeOf(r)} />
              <span class="id">{id}</span>
              <span class="t" title={r.title}>{r.title}</span>
              {#if r.status === 'blocked'}<span class="pill warn">Blocked</span>{/if}
              {#if out}
                <span class="pill"><Icon name="dep" size={12} />outside: {out.join(', ')}</span>
              {:else if r.waiting}
                <span class="pill"><Icon name="dep" size={12} />{r.waits_on.join(', ')}</span>
              {/if}
              {#if r.ready}<span class="pill ok">Ready</span>{/if}
              {#each r.repos as repo (repo)}<RepoDot id={repo} name={app.tasks.repoById.get(repo)?.name} />{/each}
            </button>
          {/if}
        {/each}
      </div>
    {/if}
  {/each}
  {#if map.tangled.length}
    <p class="tangled">These wait on each other in a loop: {map.tangled.join(', ')}.</p>
  {/if}
</div>

<style>
  .wv {
    margin-bottom: 10px;
  }
  .wv-h {
    font-size: var(--fs-label);
    color: var(--text-3);
    font-weight: 500;
    margin: 10px 0 4px;
    line-height: 14px;
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 32px;
    padding: 0 8px;
    margin: 0 -8px;
    border-radius: var(--r-md);
    width: calc(100% + 16px);
    font-size: var(--fs-body);
    line-height: 16px;
  }
  .sub:hover {
    background: var(--bg-hover);
  }
  .id {
    width: 30px;
    flex: none;
    color: var(--text-3);
    font-size: var(--fs-meta);
    font-variant-numeric: tabular-nums;
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .wait .t {
    color: var(--text-2);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 8px;
    border-radius: 11px;
    border: 1px solid var(--line-2);
    font-size: var(--fs-label);
    color: var(--text-2);
    white-space: nowrap;
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .warn {
    color: var(--st-blocked);
    border-color: color-mix(in srgb, var(--st-blocked) 35%, transparent);
  }
  .ok {
    color: var(--st-review);
    border-color: color-mix(in srgb, var(--st-review) 35%, transparent);
  }
  .tangled {
    font-size: var(--fs-meta);
    color: var(--st-blocked);
    margin-top: 4px;
  }
</style>
