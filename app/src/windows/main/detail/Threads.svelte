<script lang="ts">
  import LiveChip from '../../../lib/components/LiveChip.svelte';
  import Icon from '../../../lib/components/Icon.svelte';
  import Reply from './Reply.svelte';
  import type { LinkDto, ThreadDto } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { threadSummary } from '../../../lib/logic/copy';
  import { blocks, nextSteps } from '../../../lib/logic/reply';

  /**
   * One card per linked thread: live chip, title, request or error summary, and Open ↗ (T3
   * threads and Herdr terminals). Once the agent has stopped working, the card shows what its
   * last reply left for me, and the whole reply on request.
   */
  let { links }: { links: LinkDto[] } = $props();
  const app = useApp();
  const threads = $derived(links.filter((l): l is LinkDto & { thread: ThreadDto } => !!l.thread));
  /** Threads whose full reply is open, by link id. */
  let full = $state<Record<number, boolean>>({});

  /** While the agent works, its last reply is about the turn before, so it isn't shown. */
  function steps(t: ThreadDto) {
    if (t.state === 'running' || t.state === 'gone') return null;
    return nextSteps(t.last_reply);
  }
</script>

{#if threads.length}
  <section class="sec" aria-label="Threads">
    <h3>Threads</h3>
    {#each threads as l (l.id)}
      {@const t = l.thread}
      {@const next = steps(t)}
      <div class="card">
        <div class="thr">
          {#if t.chip}<LiveChip chip={t.chip} short />{/if}
          <span class="t"><span class="name">{t.title ?? l.ref}</span> <span class="faint">· {threadSummary(t, app.now)}</span></span>
          {#if t.url || t.key.source === 'herdr'}
            <button type="button" class="open" onclick={() => app.openThread(t.key)}>Open <Icon name="open" size={11} /></button>
          {:else if t.project_path}
            <span class="faint path" title={t.project_path}>{t.project_path}</span>
          {/if}
        </div>
        {#if next && t.last_reply}
          {@const id = `reply-${l.id}`}
          <div class="next" aria-label="What the agent left for you">
            <div class="label">
              <span>{full[l.id] ? 'Last reply' : (next.lead ?? 'Last reply')}</span>
              <button type="button" class="more" aria-expanded={!!full[l.id]} aria-controls={id} onclick={() => (full[l.id] = !full[l.id])}>
                {full[l.id] ? 'Show less' : 'Full reply'}
              </button>
            </div>
            <div {id}>
              <Reply blocks={full[l.id] ? blocks(t.last_reply) : next.blocks} />
            </div>
          </div>
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
  .card {
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    background: var(--bg-raised);
  }
  .thr {
    display: flex;
    align-items: center;
    gap: 10px;
    /* 40px with the card's border, as before replies were shown. */
    height: 38px;
    padding: 0 10px;
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
  .next {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px 12px;
    border-top: 1px solid var(--line);
  }
  .label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
  }
  .label span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .more {
    flex-shrink: 0;
    color: var(--text-3);
    font-size: var(--fs-meta);
    font-weight: 400;
    padding: 2px 6px;
    margin-right: -6px;
    border-radius: var(--r-sm);
  }
  .more:hover {
    color: var(--text);
    background: var(--bg-hover);
  }
</style>
