<script lang="ts">
  import Icon from '../../lib/components/Icon.svelte';
  import type { IconName } from '../../lib/components/icons';
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import ProgressRing from '../../lib/components/ProgressRing.svelte';
  import RepoDot from '../../lib/components/RepoDot.svelte';
  import Connections from './Connections.svelte';
  import { useApp } from '../../lib/state/context';
  import { percent, shapeOf, type View } from '../../lib/logic/views';

  const app = useApp();
  let nav: HTMLElement | undefined = $state();

  const primary: { view: View; label: string; icon: IconName; count: () => number | null }[] = [
    { view: 'needs', label: 'Needs me', icon: 'inbox', count: () => null },
    { view: 'ready', label: 'Ready to start', icon: 'play', count: () => app.counts.ready },
    { view: 'plans', label: 'Plans', icon: 'waves', count: () => app.counts.plans },
    { view: 'progress', label: 'In progress', icon: 'clock', count: () => app.counts.progress },
    { view: 'all', label: 'All tasks', icon: 'list', count: () => app.counts.all },
  ];

  const current = (v: View) => app.ui.view === v && !app.ui.pageOpen;

  /** ↑↓ move between sidebar items (roving focus); ↩ and Space open; Esc goes to the list. */
  function keydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    const items = [...(nav?.querySelectorAll<HTMLElement>('[data-nav]') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = items[Math.max(0, Math.min(items.length - 1, (i === -1 ? 0 : i) + (e.key === 'ArrowDown' ? 1 : -1)))];
      next?.focus();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      app.focus('list');
    }
  }
</script>

<aside class="side" data-zone="sidebar" aria-label="Sidebar" bind:this={app.focusTargets.sidebar} tabindex="-1">
  <div class="lights" data-tauri-drag-region>
    {#if !app.native}<i class="red"></i><i class="yellow"></i><i class="green"></i>{/if}
  </div>
  <div class="ws" data-tauri-drag-region>
    <span class="logo" aria-hidden="true"><svg width="12" height="12" viewBox="0 0 16 16"><path d="m3.5 8.3 3 3 6-6.6" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg></span>
    <b data-tauri-drag-region>Airboss</b>
    <button class="ib" type="button" title="Jump (⌘K)" aria-label="Jump" onclick={() => app.open({ kind: 'jump', mode: 'jump' })}><Icon name="search" size={15} /></button>
    <button class="ib" type="button" title="New task (C)" aria-label="New task" onclick={() => app.openNew()}><Icon name="edit" size={15} /></button>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <nav class="scroll" bind:this={nav} onkeydown={keydown} aria-label="Views">
    <div class="group">
      {#each primary as item (item.view)}
        <button type="button" class="nav" class:on={current(item.view)} data-nav aria-current={current(item.view) ? 'page' : undefined} onclick={() => app.go(item.view)}>
          <Icon name={item.icon} />
          <span class="t">{item.label}</span>
          {#if item.view === 'needs'}
            {#if app.needs.count}<span class="badge" aria-label="{app.needs.count} waiting on you">{app.needs.count}</span>{/if}
          {:else if !app.firstRun}
            <span class="c">{item.count()}</span>
          {/if}
        </button>
      {/each}
    </div>

    {#if app.parents.length}
      <div class="group">
        <div class="sech">Parents</div>
        {#each app.parents as p (p.id)}
          {@const pct = percent(p.progress)}
          <button type="button" class="nav" class:on={current(`parent:${p.id}`)} data-nav title={p.title} onclick={() => app.go(`parent:${p.id}`)}>
            <StatusIcon shape={shapeOf(p)} />
            <span class="t">{p.title}</span>
            <span class="c pc">{pct}%</span>
            <ProgressRing {pct} size={13} label={p.progress ? `${p.progress.done} of ${p.progress.total} done` : undefined} />
          </button>
        {/each}
      </div>
    {/if}

    {#if app.tasks.repos.length || !app.firstRun}
      <div class="group">
        <div class="sech">
          <span>Repos</span>
          <button class="add" type="button" title="Add repos…" aria-label="Add repos" onclick={() => app.open({ kind: 'add_repos' })}><Icon name="plus" size={12} stroke={1.6} /></button>
        </div>
        {#each app.tasks.repos as r (r.id)}
          <button type="button" class="nav" class:on={current(`repo:${r.id}`)} data-nav title={r.path} onclick={() => app.go(`repo:${r.id}`)}>
            <span class="slot"><RepoDot id={r.id} /></span>
            <span class="t">{r.name}</span>
          </button>
        {/each}
      </div>
    {/if}
  </nav>

  <Connections />
</aside>

<style>
  .side {
    background: var(--bg-side);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 0 8px;
    width: var(--sidebar-w);
    flex: none;
    outline: none;
  }
  .lights {
    display: flex;
    gap: 8px;
    height: 40px;
    align-items: center;
    padding: 0 6px;
    flex: none;
  }
  .lights i {
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }
  .red {
    background: #ff5f57;
  }
  .yellow {
    background: #febc2e;
  }
  .green {
    background: #28c840;
  }
  .ws {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 6px 10px;
    flex: none;
  }
  .logo {
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: linear-gradient(135deg, #7b7ff5, #a678f2);
    display: grid;
    place-items: center;
    flex: none;
  }
  .ws b {
    font-weight: 600;
    flex: 1;
    line-height: 16px;
  }
  .ib {
    width: 26px;
    height: 26px;
    margin: -5px -4px;
    border-radius: var(--r-md);
    display: grid;
    place-items: center;
    color: var(--text-2);
  }
  .ib + .ib {
    margin-left: 0;
  }
  .ib:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
    margin: 0 -8px;
    padding: 0 8px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 28px;
    padding: 0 8px;
    border-radius: var(--r-md);
    color: var(--text-2);
    white-space: nowrap;
    width: 100%;
    line-height: 16px;
    flex: none;
  }
  .nav:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .nav.on {
    background: var(--bg-sel);
    color: var(--text);
  }
  .nav.on .c {
    color: var(--text-2);
  }
  .nav:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 var(--kb-outline) var(--focus);
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .c {
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-meta);
    color: var(--text-3);
  }
  .pc {
    margin-right: -2px;
  }
  .badge {
    font-variant-numeric: tabular-nums;
    min-width: 18px;
    height: 18px;
    border-radius: 9px;
    background: var(--needs-fill);
    color: #fff;
    font-size: 11px;
    font-weight: 600;
    display: grid;
    place-items: center;
    padding: 0 5px;
    line-height: 14px;
  }
  .sech {
    font-size: var(--fs-label);
    font-weight: 500;
    line-height: 14px;
    color: var(--text-3);
    padding: 16px 8px 4px;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .add {
    width: 18px;
    height: 18px;
    margin: -2px -4px -2px 0;
    border-radius: var(--r-xs);
    display: grid;
    place-items: center;
    color: var(--text-3);
    opacity: 0;
  }
  .sech:hover .add,
  .add:focus-visible {
    opacity: 1;
  }
  .add:hover {
    color: var(--text);
    background: var(--bg-hover);
  }
  .slot {
    width: 16px;
    display: grid;
    place-items: center;
    flex: none;
  }
</style>
