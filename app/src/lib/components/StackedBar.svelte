<script lang="ts">
  import type { Progress } from '../types';

  /** Done, in review, in progress, blocked; todo is the empty track. 6px (8px on the page). */
  let {
    p,
    height = 6,
    surface = 'var(--bg)',
  }: { p: Progress; height?: number; surface?: string } = $props();
  const parts = $derived(
    (
      [
        ['done', 'var(--st-done)'],
        ['in_review', 'var(--st-review)'],
        ['doing', 'var(--st-doing)'],
        ['blocked', 'var(--st-blocked)'],
      ] as const
    )
      .map(([k, color]) => ({ k, color, w: p.total ? (p[k] / p.total) * 100 : 0 }))
      .filter((x) => x.w > 0),
  );
</script>

<span
  class="bar"
  style:height="{height}px"
  style:border-radius="{height / 2}px"
  style:--surface={surface}
  role="img"
  aria-label="{p.done} of {p.total} done, {p.in_review} in review, {p.doing} in progress, {p.blocked} blocked"
>
  {#each parts as part, i (part.k)}
    <i class:sep={i > 0} style:width="{part.w}%" style:background={part.color}></i>
  {/each}
</span>

<style>
  .bar {
    display: flex;
    flex: 1;
    min-width: 40px;
    overflow: hidden;
    background: var(--bg-input);
  }
  i {
    display: block;
    height: 100%;
    transition: width var(--dur-progress) var(--ease);
  }
  .sep {
    box-shadow: inset 1px 0 0 var(--surface);
  }
</style>
