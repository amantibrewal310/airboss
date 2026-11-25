<script lang="ts">
  import Icon from '../../../lib/components/Icon.svelte';
  import type { NeedsItem } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { KIND_ICON, KIND_LABEL, mainLabel, otherLabel } from '../../../lib/logic/needs';
  import { relative } from '../../../lib/logic/time';

  /** One red-washed callout per Needs me item, with its main action and Dismiss. */
  let { items }: { items: NeedsItem[] } = $props();
  const app = useApp();

  function other(n: NeedsItem) {
    if (n.kind === 'quiet') return app.snooze(n);
    if (n.kind === 'proposed') return app.rejectProposals(n, n.proposed.map(([id]) => id));
    return app.dismiss(n, n.kind === 'finish_it' ? 'Kept open' : 'Dismissed');
  }
</script>

{#each items as n (n.key)}
  <div class="call" class:stale={n.stale} class:gone={app.needs.leaving.has(n.key)}>
    <span class="ic"><Icon name={KIND_ICON[n.kind]} size={15} /></span>
    <div class="w">
      <div class="k">{KIND_LABEL[n.kind]} · {relative(n.since, app.now)} ago{#if n.stale}<span class="st">{' · May be out of date'}</span>{/if}</div>
      <div class="q">{n.headline}</div>
      <div class="acts">
        <button type="button" class="pri" onclick={() => app.mainAction(n)}>{mainLabel(n.kind)}</button>
        <button type="button" class="sec" onclick={() => other(n)}>{otherLabel(n.kind)}</button>
      </div>
    </div>
  </div>
{/each}

<style>
  .call {
    display: flex;
    gap: 10px;
    padding: 11px 12px;
    border-radius: var(--r-lg);
    background: var(--needs-soft);
    border: 1px solid var(--needs-edge);
    transition:
      opacity var(--dur-exit) var(--ease),
      transform var(--dur-exit) var(--ease);
  }
  .gone {
    opacity: 0;
    transform: translateX(-10px);
  }
  /* Out of date: a dashed edge and the label say so; the text keeps its contrast. */
  .stale {
    border-style: dashed;
  }
  .ic {
    color: var(--needs);
    padding-top: 1px;
    flex: none;
  }
  .w {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .k {
    font-size: var(--fs-label);
    font-weight: 600;
    line-height: 14px;
    color: var(--needs-text);
    font-variant-numeric: tabular-nums;
  }
  .st {
    font-weight: 500;
    color: var(--text-2);
  }
  .q {
    color: var(--text);
    line-height: 19px;
  }
  .acts {
    display: flex;
    gap: 6px;
    padding-top: 7px;
  }
  button {
    height: 28px;
    padding: 0 11px;
    border-radius: var(--r-md);
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
    border: 1px solid transparent;
  }
  .pri {
    background: var(--accent-fill);
    color: var(--on-fill);
  }
  .pri:hover {
    filter: brightness(1.1);
  }
  .sec {
    background: var(--bg-input);
    border-color: var(--line-2);
    color: var(--text);
  }
  .sec:hover {
    background: var(--bg-sel);
  }
</style>
