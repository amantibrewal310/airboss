<script lang="ts">
  import Icon from '../../lib/components/Icon.svelte';
  import Button from '../../lib/components/Button.svelte';
  import type { NeedsItem } from '../../lib/types';
  import { useApp } from '../../lib/state/context';
  import { grouped, KIND_ICON } from '../../lib/logic/needs';
  import { clockTime, relative } from '../../lib/logic/time';
  import NeedsCard from './NeedsCard.svelte';

  /** Needs me: the 420px inbox, grouped in the fixed order, and the selected item's card and task. */
  const app = useApp();
  const groups = $derived(grouped(app.needs.items.filter((n) => !app.needs.hidden.has(n.key))));
  const item = $derived(app.needsItem);

  function pick(n: NeedsItem) {
    app.selectNeed(n.key, false);
  }
</script>

{#if app.needs.count === 0}
  <div class="empty" bind:this={app.focusTargets.list} tabindex="-1">
    <svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9.25" fill="none" stroke="var(--text-2)" stroke-width="1.5" /><path d="m8 12.3 2.8 2.8 5.2-5.6" fill="none" stroke="var(--text-2)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
    <h2>Nothing needs you</h2>
    <p>
      {[app.needs.checkedAt ? `Checked ${clockTime(app.needs.checkedAt, app.now)}` : '', `${app.counts.progress} in progress`, `${app.counts.ready} ready to start`]
        .filter(Boolean)
        .join(' · ')}
    </p>
    <Button onclick={() => app.go('ready')}>Show ready tasks</Button>
  </div>
{:else}
  <div
    class="inbox"
    bind:this={app.focusTargets.list}
    role="listbox"
    tabindex="0"
    aria-label="Needs me"
    aria-activedescendant={item ? `need-${item.key}` : undefined}
  >
    {#each groups as g (g.group)}
      <div class="ib-h" role="presentation"><span>{g.label}</span><span>{g.items.length}</span></div>
      {#each g.items as n (n.key)}
        <div
          id="need-{n.key}"
          class="ib"
          class:sel={item?.key === n.key}
          class:kb={item?.key === n.key && app.ui.keyboard && (app.ui.zone === 'list' || app.ui.zone === 'overlay')}
          class:stale={n.stale}
          class:gone={app.needs.leaving.has(n.key)}
          role="option"
          aria-selected={item?.key === n.key}
          tabindex="-1"
          onclick={() => pick(n)}
          onkeydown={() => {}}
        >
          <span class="ic"><Icon name={KIND_ICON[n.kind]} size={15} /></span>
          <div class="w">
            <div class="l1"><span class="id">{n.task}</span><span class="tt">{n.task_title}</span><span class="tm">{relative(n.since, app.now)}</span></div>
            <div class="l2">{n.stale ? `May be out of date · ${n.context}` : n.context}</div>
          </div>
        </div>
      {/each}
    {/each}
  </div>
  <div class="detail" data-zone="panel" bind:this={app.focusTargets.panel} tabindex="-1">
    {#if item}<NeedsCard {item} />{/if}
  </div>
{/if}

<style>
  .inbox {
    width: 420px;
    flex: none;
    border-right: 1px solid var(--line);
    overflow-y: auto;
    scrollbar-width: thin;
    outline: none;
  }
  .ib-h {
    font-size: var(--fs-label);
    font-weight: 500;
    line-height: 14px;
    color: var(--text-3);
    padding: 14px 16px 6px;
    display: flex;
    gap: 6px;
    font-variant-numeric: tabular-nums;
  }
  .ib {
    display: flex;
    gap: 10px;
    padding: 10px 14px 10px 16px;
    border-bottom: 1px solid var(--line);
    position: relative;
    line-height: 16px;
    transition:
      opacity var(--dur-exit) var(--ease),
      transform var(--dur-exit) var(--ease);
  }
  .ib:hover {
    background: var(--bg-hover);
  }
  .ib.sel {
    background: var(--bg-sel);
  }
  .ib.kb::after {
    content: '';
    position: absolute;
    inset: 0;
    box-shadow: inset 0 0 0 var(--kb-outline) var(--focus);
    pointer-events: none;
  }
  .ib.gone {
    opacity: 0;
    transform: translateX(-10px);
  }
  .ib.stale .w {
    opacity: 0.6;
  }
  .ic {
    width: 18px;
    flex: none;
    padding-top: 1px;
    color: var(--text-2);
  }
  .w {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .l1 {
    display: flex;
    gap: 7px;
    align-items: center;
    min-width: 0;
  }
  .id {
    font-size: var(--fs-meta);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .tt {
    flex: 1;
    min-width: 0;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tm {
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .l2 {
    color: var(--text-2);
    font-size: var(--fs-meta);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    outline: none;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding-top: 220px;
    color: var(--text-2);
    outline: none;
    text-align: center;
  }
  .empty h2 {
    font-size: var(--fs-title);
    font-weight: 600;
    line-height: 24px;
    color: var(--text);
    margin: 10px 0 2px;
  }
  .empty p {
    margin-bottom: 18px;
    font-variant-numeric: tabular-nums;
  }
</style>
