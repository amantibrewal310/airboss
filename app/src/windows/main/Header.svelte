<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import Segmented from '../../lib/components/Segmented.svelte';
  import { useApp } from '../../lib/state/context';
  import { clockTime } from '../../lib/logic/time';
  import type { Layout } from '../../lib/logic/views';

  /** 44px, a drag region: title and count, then the view's controls, Jump ⌘K and the primary action. */
  const app = useApp();
  const kind = $derived(app.viewKind);
  const planTotals = $derived(
    app.plans.reduce((a, c) => ({ done: a.done + c.progress.done, total: a.total + c.progress.total }), { done: 0, total: 0 }),
  );
</script>

<header class="head" data-tauri-drag-region>
  <h1 class="title" data-tauri-drag-region>{app.title}</h1>
  {#if kind === 'needs'}
    {#if app.needs.count}<span class="cnt" data-tauri-drag-region>{app.needs.count}</span>{/if}
  {:else if kind === 'plans'}
    <span class="cnt" data-tauri-drag-region>{app.plans.length}</span>
  {:else if !app.firstRun}
    <span class="cnt" data-tauri-drag-region>{app.headerCount}</span>
  {/if}
  <span class="sp" data-tauri-drag-region></span>

  {#if kind === 'needs'}
    {#if app.needs.count && app.needs.checkedAt}
      <span class="meta">checked {clockTime(app.needs.checkedAt, app.now)}</span>
    {/if}
    {#if app.needs.count}
      <Button variant="chip" onclick={() => app.open({ kind: 'jump', mode: 'jump' })}><Icon name="search" size={13} />Jump <Kbd keys="mod+k" joined /></Button>
    {/if}
  {:else if kind === 'plans'}
    {#if planTotals.total}
      <span class="meta">{Math.round((planTotals.done / planTotals.total) * 100)}% of all planned work done · {planTotals.done} of {planTotals.total}</span>
    {/if}
    <Button variant="primary" onclick={() => app.openNew({ plan: true })}><Icon name="waves" size={13} />Plan something</Button>
  {:else}
    {#if app.ui.view === 'all'}
      <Segmented
        label="Layout"
        options={[
          { value: 'tree', label: 'Tree' },
          { value: 'status', label: 'By status' },
        ] as { value: Layout; label: string }[]}
        value={app.ui.layout}
        onchange={(v) => app.setLayout(v)}
      />
    {/if}
    <Button variant="chip" onclick={() => app.open({ kind: 'jump', mode: 'jump' })}><Icon name="search" size={13} />Jump <Kbd keys="mod+k" joined /></Button>
    <Button variant="primary" onclick={() => app.openNew()}><Icon name="edit" size={13} />New task</Button>
  {/if}
</header>

<style>
  .head {
    height: var(--header-h);
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 14px 0 18px;
    border-bottom: 1px solid var(--line);
    min-width: 0;
  }
  .title {
    font-size: var(--fs-body);
    font-weight: 600;
    line-height: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .cnt {
    color: var(--text-3);
    line-height: 16px;
  }
  .sp {
    flex: 1;
    align-self: stretch;
  }
  .meta {
    font-size: var(--fs-meta);
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
