<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import type { T3ProjectRow } from '../../lib/types';
  import { useApp } from '../../lib/state/context';

  /**
   * Add repos from T3's projects (each unticked; folders that aren't git repos or are already
   * added are greyed with the reason), or another folder. ↑↓ move, Space ticks, ⌘↩ adds.
   */
  let { embedded = false, ondone }: { embedded?: boolean; ondone?: () => void } = $props();
  const app = useApp();
  let rows = $state<T3ProjectRow[]>([]);
  let loaded = $state(false);
  let ticked = $state<string[]>([]);
  let error = $state<string | null>(null);
  let list: HTMLElement | undefined = $state();

  onMount(async () => {
    rows = await app.t3Projects();
    loaded = true;
  });

  const reason = (r: T3ProjectRow) => (r.added ? 'Already added' : !r.is_git ? 'Not a git repository' : null);
  const t3 = $derived(app.conn.t3Ready && (rows.length > 0 || !loaded));

  function toggle(r: T3ProjectRow) {
    if (reason(r)) return;
    ticked = ticked.includes(r.path) ? ticked.filter((p) => p !== r.path) : [...ticked, r.path];
  }

  async function add() {
    if (!ticked.length) return;
    error = await app.addRepos(ticked);
    if (!error) {
      ticked = [];
      rows = await app.t3Projects();
      ondone?.();
    }
  }

  async function another() {
    const path = await app.pickFolder();
    if (!path) return;
    error = await app.addRepos([path]);
    if (!error) ondone?.();
  }

  function key(e: KeyboardEvent) {
    const items = [...(list?.querySelectorAll<HTMLElement>('[role="checkbox"]') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopPropagation();
      items[Math.max(0, Math.min(items.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))]?.focus();
    } else if (e.key === 'Enter' && e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      void add();
    }
  }
</script>

<div class="ar" class:embedded>
  {#if t3 && rows.length}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="list" bind:this={list} role="group" aria-label="T3 Code projects" tabindex="-1" onkeydown={key}>
      {#each rows as r (r.path)}
        {@const why = reason(r)}
        <div
          class="r"
          class:off={!!why}
          role="checkbox"
          aria-checked={ticked.includes(r.path)}
          aria-disabled={why ? 'true' : undefined}
          tabindex="0"
          title={why ?? r.path}
          onclick={() => toggle(r)}
          onkeydown={(e) => {
            if (e.key === ' ') {
              e.preventDefault();
              e.stopPropagation();
              toggle(r);
            }
          }}
        >
          <span class="box" aria-hidden="true">{#if ticked.includes(r.path)}<Icon name="check" size={12} stroke={2} />{/if}</span>
          <span class="name">{r.name}</span>
          <span class="path">{r.path}</span>
          {#if why}<span class="why">{why}</span>{/if}
        </div>
      {/each}
    </div>
  {:else if loaded}
    <p class="none">T3 Code's projects show here once it's installed.</p>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <div class="acts">
    <Button variant="ghost" onclick={another}><Icon name="folder" size={14} />Add another folder…</Button>
    <span class="sp"></span>
    {#if t3 && rows.length}
      <Button variant={embedded ? 'secondary' : 'primary'} disabled={!ticked.length} onclick={add}>
        Add {ticked.length || ''} {ticked.length === 1 ? 'repo' : 'repos'}
      </Button>
    {/if}
  </div>
</div>

<style>
  .ar {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .list {
    border: 1px solid var(--line);
    border-radius: var(--r-pop);
    background: var(--bg-raised);
    overflow: hidden;
    outline: none;
  }
  .r {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 38px;
    padding: 0 14px;
    border-bottom: 1px solid var(--line);
    outline: none;
  }
  .r:last-child {
    border-bottom: 0;
  }
  .r:hover {
    background: var(--bg-hover);
  }
  .r:focus-visible {
    box-shadow: inset 0 0 0 var(--kb-outline) var(--focus);
  }
  .r.off {
    color: var(--text-3);
  }
  .box {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1px solid var(--line-2);
    background: var(--bg-input);
    display: grid;
    place-items: center;
    color: var(--on-fill);
    flex: none;
  }
  [aria-checked='true'] .box {
    background: var(--accent-fill);
    border-color: transparent;
  }
  .name {
    font-weight: 500;
  }
  .off .name {
    font-weight: 400;
  }
  .path {
    color: var(--text-3);
    font-size: var(--fs-meta);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
  .why {
    font-size: var(--fs-label);
    color: var(--text-3);
    white-space: nowrap;
  }
  .none {
    color: var(--text-2);
    font-size: var(--fs-meta);
  }
  .err {
    color: var(--st-blocked);
    font-size: var(--fs-meta);
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .acts :global(.ghost) {
    margin-left: -10px;
  }
  .sp {
    flex: 1;
  }
</style>
