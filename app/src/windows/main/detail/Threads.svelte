<script lang="ts">
  import LiveChip from '../../../lib/components/LiveChip.svelte';
  import Icon from '../../../lib/components/Icon.svelte';
  import type { LinkDto, ThreadDto } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { threadSummary } from '../../../lib/logic/copy';

  /** One row per linked thread: live chip, title, request or error summary, and Open ↗ (T3 threads and Herdr terminals). */
  let { links }: { links: LinkDto[] } = $props();
  const app = useApp();
  const threads = $derived(links.filter((l): l is LinkDto & { thread: ThreadDto } => !!l.thread));

</script>

{#if threads.length}
  <section class="sec" aria-label="Threads">
    <h3>Threads</h3>
    {#each threads as l (l.id)}
      {@const t = l.thread}
      <div class="thr">
        {#if t.chip}<LiveChip chip={t.chip} short />{/if}
        <span class="t"><span class="name">{t.title ?? l.ref}</span> <span class="faint">· {threadSummary(t, app.now)}</span></span>
        {#if t.url || t.key.source === 'herdr'}
          <button type="button" class="open" onclick={() => app.openThread(t.key)}>Open <Icon name="open" size={11} /></button>
        {:else if t.project_path}
          <span class="faint path" title={t.project_path}>{t.project_path}</span>
        {/if}
      </div>
    {/each}
  </section>
{/if}

<style>
  .sec {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 6px;
  }
  h3 {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
  }
  .thr {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    background: var(--bg-raised);
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-control);
    line-height: 16px;
  }
  .faint {
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .open {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text-2);
    font-size: var(--fs-meta);
    padding: 4px 6px;
    margin-right: -4px;
    border-radius: var(--r-sm);
  }
  .open:hover {
    color: var(--text);
    background: var(--bg-hover);
  }
  .path {
    font-family: var(--mono);
    font-size: var(--fs-label);
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
