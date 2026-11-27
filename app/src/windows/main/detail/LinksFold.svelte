<script lang="ts">
  import Icon from '../../../lib/components/Icon.svelte';
  import type { LinkDto } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { linkLabel } from '../../../lib/logic/links';

  /** A parent's links, folded under its notes: "Links 3", opening to the tags with ✕ and ⌘L. */
  let { links }: { links: LinkDto[] } = $props();
  const app = useApp();
  const uid = $props.id();
  let open = $state(false);
</script>

<div class="fold">
  <button type="button" class="head" aria-expanded={open} aria-controls="links-{uid}" onclick={() => (open = !open)}>
    <span class="chev" class:open><Icon name="chevron" size={12} /></span>Links <span class="n">{links.length}</span>
  </button>
  <div id="links-{uid}" class="tags" hidden={!open}>
    {#if open}
      {#each links as l (l.id)}
        <span class="tag" title={l.url ?? l.ref}>
          <button type="button" class="in" onclick={() => app.openLink(l)}>{linkLabel(l)}</button>
          <button type="button" class="x" aria-label="Unlink {linkLabel(l)}" onclick={() => app.removeLink(l.id)}><Icon name="close" size={11} stroke={1.8} /></button>
        </span>
      {/each}
      <button type="button" class="add" onclick={() => app.runCommand('add_link')}>+ Add link · ⌘L</button>
    {/if}
  </div>
</div>

<style>
  .head {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
    margin-left: -4px;
    padding: 2px 4px;
    border-radius: var(--r-md);
  }
  .head:hover {
    color: var(--text);
  }
  .n {
    color: var(--text-3);
    font-weight: 500;
  }
  /* The chevron points down; folded, it turns to point right. */
  .chev {
    display: inline-grid;
    transform: rotate(-90deg);
    transition: transform var(--dur-panel) var(--ease);
  }
  .chev.open {
    transform: none;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding-top: 8px;
  }
  .tags[hidden] {
    display: none;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    height: 22px;
    border-radius: var(--r-sm);
    background: var(--bg-input);
    border: 1px solid var(--line);
    font-size: var(--fs-meta);
    color: var(--text);
    white-space: nowrap;
    max-width: 100%;
  }
  .tag:hover {
    border-color: var(--line-2);
  }
  .in {
    height: 100%;
    padding: 0 7px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .x {
    display: none;
    height: 100%;
    padding: 0 6px 0 0;
    margin-left: -3px;
    color: var(--text-3);
    align-items: center;
  }
  .tag:hover .x,
  .tag:focus-within .x {
    display: inline-flex;
  }
  .x:hover {
    color: var(--text);
  }
  .add {
    color: var(--text-3);
    font-size: var(--fs-meta);
    padding-left: 4px;
  }
  .add:hover {
    color: var(--text-2);
  }
</style>
