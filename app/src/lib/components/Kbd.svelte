<script lang="ts">
  import { capsOf } from '../logic/keymap';

  /** Key caps: one cap per key ("mod+enter" → ⌘ ↩). `onFill` for caps inside a primary button. */
  let { keys, onFill = false, joined = false }: { keys: string; onFill?: boolean; joined?: boolean } = $props();
  const caps = $derived(capsOf(keys));
</script>

{#if joined}
  <kbd class="cap" class:fill={onFill}>{caps.join('')}</kbd>
{:else}
  <span class="caps">
    {#each caps as cap, i (i)}
      <kbd class="cap" class:fill={onFill}>{cap}</kbd>
    {/each}
  </span>
{/if}

<style>
  .caps {
    display: inline-flex;
    gap: 3px;
  }
  .cap {
    font: 400 var(--fs-kbd) / 14px var(--font);
    color: var(--text-2);
    background: var(--bg-input);
    border: 1px solid var(--line-2);
    border-radius: var(--r-xs);
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    display: inline-grid;
    place-items: center;
    white-space: nowrap;
    letter-spacing: 0;
  }
  .fill {
    background: var(--on-fill-kbd);
    border-color: transparent;
    color: var(--on-fill);
    font-weight: 500;
  }
</style>
