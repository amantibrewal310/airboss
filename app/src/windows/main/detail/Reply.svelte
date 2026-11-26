<script lang="ts">
  import type { Block, Inline } from '../../../lib/logic/reply';
  import { useApp } from '../../../lib/state/context';

  /** An agent's words as text: paragraphs, headings and lists, with bold, code and https links. Never HTML. */
  let { blocks }: { blocks: Block[] } = $props();
  const app = useApp();
</script>

{#snippet parts(list: Inline[])}
  {#each list as p, i (i)}
    {#if p.kind === 'bold'}<strong>{p.text}</strong>
    {:else if p.kind === 'code'}<code>{p.text}</code>
    {:else if p.kind === 'link'}<button type="button" class="a" title={p.url} onclick={() => app.openExternal(p.url)}>{p.text}</button>
    {:else}{p.text}{/if}
  {/each}
{/snippet}

<div class="reply">
  {#each blocks as b, i (i)}
    {#if b.kind === 'heading'}
      <p class="h">{@render parts(b.parts)}</p>
    {:else if b.kind === 'para'}
      <p>{@render parts(b.parts)}</p>
    {:else if b.ordered}
      <ol>{#each b.items as item, j (j)}<li>{@render parts(item)}</li>{/each}</ol>
    {:else}
      <ul>{#each b.items as item, j (j)}<li>{@render parts(item)}</li>{/each}</ul>
    {/if}
  {/each}
</div>

<style>
  .reply {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--fs-control);
    line-height: 19px;
    color: var(--text);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .h {
    font-weight: 600;
  }
  ul,
  ol {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin: 0;
    padding-left: 18px;
  }
  ul {
    list-style: disc;
  }
  ol {
    list-style: decimal;
  }
  li::marker {
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  strong {
    font-weight: 600;
  }
  code {
    font-family: var(--mono);
    font-size: 0.92em;
    padding: 0 3px;
    border-radius: var(--r-sm);
    background: var(--bg-hover);
  }
  .a {
    display: inline;
    padding: 0;
    color: var(--link);
    font: inherit;
    text-align: left;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .a:hover {
    text-decoration-color: var(--link);
  }
</style>
