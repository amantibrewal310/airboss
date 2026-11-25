<script lang="ts">
  /**
   * Indigo arc on a line-2 track; green at 100%. 13–14px in rows and the sidebar, 22px on cards.
   * `label` names it ("3 of 5 done"); without one it's decorative (the row or card says it).
   */
  let { pct, size = 13, label }: { pct: number; size?: number; label?: string } = $props();
  const r = $derived(size / 2 - 1.6);
  const c = $derived(2 * Math.PI * r);
  const full = $derived(pct >= 100);
</script>

<svg
  class="ring"
  width={size}
  height={size}
  viewBox="0 0 {size} {size}"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
>
  <circle cx={size / 2} cy={size / 2} {r} fill="none" stroke={full ? 'var(--st-review)' : 'var(--line-2)'} stroke-width="2" />
  {#if !full && pct > 0}
    <circle
      class="arc"
      cx={size / 2}
      cy={size / 2}
      {r}
      fill="none"
      stroke="var(--st-done)"
      stroke-width="2"
      stroke-linecap="round"
      stroke-dasharray="{(c * pct) / 100} {c}"
      transform="rotate(-90 {size / 2} {size / 2})"
    />
  {/if}
</svg>

<style>
  .ring {
    flex: none;
    display: block;
  }
  .arc {
    transition: stroke-dasharray var(--dur-progress) var(--ease);
  }
</style>
