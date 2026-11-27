<script lang="ts">
  import { fullTitle } from '../../../lib/actions/fullTitle';
  import StatusIcon from '../../../lib/components/StatusIcon.svelte';
  import RepoDot from '../../../lib/components/RepoDot.svelte';
  import Icon from '../../../lib/components/Icon.svelte';
  import type { TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { shapeOf } from '../../../lib/logic/views';

  /** 32px step rows: status, id, title, what it waits on, repo dots. Click reveals; double-click opens. */
  let { ids, outside = new Map() }: { ids: TaskId[]; outside?: Map<TaskId, TaskId[]> } = $props();
  const app = useApp();
</script>

{#each ids as id (id)}
  {@const r = app.row(id)}
  {#if r}
    {@const out = outside.get(id)}
    <button type="button" class="sub" class:wait={r.waiting || !!out} onclick={() => app.reveal(id)} ondblclick={() => app.openPage(id)}>
      <StatusIcon shape={shapeOf(r)} />
      <span class="id">{id}</span>
      <span class="t" use:fullTitle={r.title}>{r.title}</span>
      {#if r.status === 'blocked'}
        <span class="pill warn">Blocked</span>
      {:else if out}
        <span class="pill"><Icon name="dep" size={12} />outside: {out.join(', ')}</span>
      {:else if r.waiting}
        <span class="pill"><Icon name="dep" size={12} />{r.waits_on.join(', ')}</span>
      {/if}
      {#each r.repos as repo (repo)}<RepoDot id={repo} name={app.tasks.repoById.get(repo)?.name} />{/each}
    </button>
  {/if}
{/each}

<style>
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
    color: var(--warn-text);
    border-color: color-mix(in srgb, var(--st-blocked) 35%, transparent);
  }
</style>
