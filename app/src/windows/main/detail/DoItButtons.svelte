<script lang="ts">
  import Icon from '../../../lib/components/Icon.svelte';
  import Kbd from '../../../lib/components/Kbd.svelte';
  import { useApp } from '../../../lib/state/context';
  import type { TaskId } from '../../../lib/types';

  /** Plan it (⇧⌘↩) and Do it (primary, ⌘↩), with "Starting…" and the T3-missing state. */
  let { task }: { task: TaskId } = $props();
  const app = useApp();
  const row = $derived(app.row(task));
  const off = $derived(!app.conn.t3Ready);
  const starting = $derived(app.ui.starting.has(task));
  const closed = $derived(row?.status === 'done' || row?.status === 'dropped' || row?.status === 'proposed');
</script>

<button
  type="button"
  class="ghost"
  aria-disabled={off || closed ? 'true' : undefined}
  title={off ? 'Do it and Plan it need T3 Code' : 'Plan it (⇧⌘↩)'}
  onclick={() => app.planIt(task)}
>
  <Icon name="waves" size={14} />Plan it
</button>
<button
  type="button"
  class="pri"
  data-do-it
  aria-disabled={off || starting ? 'true' : undefined}
  aria-busy={starting}
  title={off ? 'Do it and Plan it need T3 Code' : 'Do it (⌘↩)'}
  onclick={() => !starting && app.doIt(task)}
>
  {#if starting}
    <span class="spin" aria-hidden="true"></span>Starting…
  {:else}
    <Icon name="play" size={14} />Do it <Kbd keys="mod+enter" onFill joined />
  {/if}
</button>

<style>
  button {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 0 10px;
    border-radius: var(--r-md);
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    flex: none;
  }
  .ghost {
    color: var(--text-2);
  }
  .ghost:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .pri {
    background: var(--accent-fill);
    color: var(--on-fill);
  }
  .pri:hover:not([aria-disabled='true']) {
    filter: brightness(1.1);
  }
  [aria-disabled='true'] {
    opacity: 0.45;
  }
  .pri[aria-busy='true'] {
    opacity: 0.8;
  }
  .ghost[aria-disabled='true']:hover {
    background: none;
    color: var(--text-2);
  }
  .spin {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--on-fill) 35%, transparent);
    border-top-color: var(--on-fill);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
