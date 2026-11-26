<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { fullTitle } from '../../lib/actions/fullTitle';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import type { T3ProjectRow } from '../../lib/types';
  import { useApp } from '../../lib/state/context';

  /**
   * Add repos from T3's projects, or without T3 Code from Herdr's space and recent folders
   * ("From Herdr"), each unticked; folders that aren't git repos or are already added are
   * greyed with the reason. Or another folder. The list is one Tab stop (roving
   * tabindex): ↑↓ move, Home/End jump, Space ticks, ⌘↩ adds. Embedded in first run, it stays
   * after an add: added repos show ticked and greyed, and other ticks are kept.
   */
  let { embedded = false, ondone }: { embedded?: boolean; ondone?: () => void } = $props();
  const app = useApp();
  const uid = $props.id();
  let rows = $state<T3ProjectRow[]>([]);
  let loaded = $state(false);
  let ticked = $state<string[]>([]);
  let error = $state<string | null>(null);
  let list: HTMLElement | undefined = $state();
  let acts: HTMLElement | undefined = $state();
  /** The row that holds the list's one Tab stop. */
  let active = $state<string | null>(null);

  /** Without T3 Code, Herdr's folders stand in for its projects. Chosen once, so the list doesn't swap under the user. */
  const fromHerdr = untrack(() => !app.conn.t3Ready && typeof app.conn.herdr === 'object' && 'ready' in app.conn.herdr);
  const load = () => (fromHerdr ? app.herdrFolders() : app.t3Projects());
  onMount(async () => {
    rows = await load();
    loaded = true;
  });

  // First run lists repos added from other folders too, so every add stays visible.
  const items = $derived.by((): T3ProjectRow[] => {
    if (!embedded) return rows;
    const extra = app.tasks.repos
      .filter((r) => !rows.some((x) => x.path === r.path))
      .map((r) => ({ path: r.path, name: r.name, is_git: true, added: true }));
    return [...rows, ...extra];
  });
  const reason = (r: T3ProjectRow) => (r.added ? (embedded ? 'Added' : 'Already added') : !r.is_git ? 'Not a git repository' : null);
  const listed = $derived((fromHerdr || app.conn.t3Ready) && (rows.length > 0 || !loaded));
  const current = $derived(items.some((r) => r.path === active) ? active : (items[0]?.path ?? null));
  const id = (i: number, part: string) => `${uid}-${i}-${part}`;

  function toggle(r: T3ProjectRow) {
    active = r.path;
    if (reason(r)) return;
    ticked = ticked.includes(r.path) ? ticked.filter((p) => p !== r.path) : [...ticked, r.path];
  }

  /** Reads the projects (or Herdr's folders) again and keeps only the ticks that can still be added. */
  async function reload() {
    rows = await load();
    const addable = new Set(rows.filter((r) => !reason(r)).map((r) => r.path));
    ticked = ticked.filter((p) => addable.has(p));
  }

  function focusRow(path: string | null) {
    active = path;
    void tick().then(() => list?.querySelector<HTMLElement>('[tabindex="0"][role="checkbox"]')?.focus());
  }

  async function add() {
    if (!ticked.length) return;
    error = await app.addRepos(ticked);
    if (!error) {
      ticked = [];
      await reload();
      ondone?.();
      // "Add N repos" is disabled now; the list keeps focus.
      if (embedded) focusRow(current);
    }
  }

  async function another() {
    const path = await app.pickFolder();
    if (path) {
      error = await app.addRepos([path]);
      if (!error) {
        await reload();
        ondone?.();
      }
    }
    // The folder picker took focus; give it back to the button that opened it.
    await tick();
    acts?.querySelector<HTMLElement>('button')?.focus();
  }

  function key(e: KeyboardEvent) {
    const at = items.findIndex((r) => r.path === current);
    let to: number | null = null;
    if (e.key === 'ArrowDown') to = Math.min(items.length - 1, at + 1);
    else if (e.key === 'ArrowUp') to = Math.max(0, at - 1);
    else if (e.key === 'Home') to = 0;
    else if (e.key === 'End') to = items.length - 1;
    else if (e.key === ' ' && at >= 0) {
      e.preventDefault();
      e.stopPropagation();
      toggle(items[at]!);
      return;
    } else if (e.key === 'Enter' && e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      void add();
      return;
    }
    if (to === null) return;
    e.preventDefault();
    e.stopPropagation();
    focusRow(items[to]?.path ?? null);
  }
</script>

<div class="ar" class:embedded>
  {#if fromHerdr && rows.length}<h3 class="from" id="{uid}-from">From Herdr</h3>{/if}
  {#if items.length && (listed || embedded)}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="list"
      bind:this={list}
      role="group"
      aria-label={fromHerdr && rows.length ? undefined : rows.length ? 'T3 Code projects' : 'Repos'}
      aria-labelledby={fromHerdr && rows.length ? `${uid}-from` : undefined}
      onkeydown={key}
    >
      {#each items as r, i (r.path)}
        {@const why = reason(r)}
        <!-- Space and the arrows are the list's (roving tabindex), so the row needs no handler of its own. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="r"
          class:off={!!why}
          role="checkbox"
          aria-checked={r.added || ticked.includes(r.path)}
          aria-disabled={why ? 'true' : undefined}
          aria-label={r.name}
          aria-describedby="{id(i, 'path')}{why ? ` ${id(i, 'why')}` : ''}"
          tabindex={r.path === current ? 0 : -1}
          onclick={() => toggle(r)}
          onfocus={() => (active = r.path)}
        >
          <span class="box" aria-hidden="true">{#if r.added || ticked.includes(r.path)}<Icon name="check" size={12} stroke={2} />{/if}</span>
          <span class="name">{r.name}</span>
          <span class="path" id={id(i, 'path')} use:fullTitle={r.path}>{r.path}</span>
          {#if why}<span class="why" id={id(i, 'why')}>{why}</span>{/if}
        </div>
      {/each}
    </div>
  {/if}
  {#if loaded && !listed}
    <p class="none">{fromHerdr ? "Herdr's folders show here while it's open." : "T3 Code's projects show here once it's installed."}</p>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <div class="acts" bind:this={acts}>
    <Button variant="ghost" onclick={another}><Icon name="folder" size={14} />Add another folder…</Button>
    <span class="sp"></span>
    {#if listed && rows.length}
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
    /* In the modal, the list shrinks and scrolls so the buttons stay in the window. */
    flex: 1 1 auto;
    min-height: 0;
  }
  .list {
    border: 1px solid var(--line);
    border-radius: var(--r-pop);
    background: var(--bg-raised);
    overflow: hidden;
    outline: none;
  }
  .ar:not(.embedded) .list {
    overflow-y: auto;
    min-height: 0;
    flex: 0 1 auto;
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
  .off[aria-checked='true'] .box {
    opacity: 0.55;
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
  .from {
    font-size: var(--fs-label);
    font-weight: 500;
    line-height: 14px;
    color: var(--text-3);
  }
  .none {
    color: var(--text-2);
    font-size: var(--fs-meta);
  }
  .err {
    color: var(--warn-text);
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
