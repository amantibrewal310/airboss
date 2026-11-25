<script lang="ts">
  import type { Snippet } from 'svelte';

  /** A property value: 22px, 5px corners, Field Grey. A button when it does something. */
  let {
    onclick,
    title,
    mono = false,
    children,
  }: { onclick?: (e: MouseEvent) => void; title?: string; mono?: boolean; children: Snippet } = $props();
</script>

{#if onclick}
  <button type="button" class="tag link" class:mono {title} {onclick}>{@render children()}</button>
{:else}
  <span class="tag" class:mono {title}>{@render children()}</span>
{/if}

<style>
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 7px;
    border-radius: var(--r-sm);
    background: var(--bg-input);
    border: 1px solid var(--line);
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text);
    white-space: nowrap;
    max-width: 100%;
    min-width: 0;
    flex: none;
  }
  .link:hover {
    border-color: var(--line-2);
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--fs-label);
  }
</style>
