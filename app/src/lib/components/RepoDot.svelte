<script lang="ts">
  /** A repo's colour, fixed by id: repo-((id − 1) mod 4) + 1 (the Owned Colour Rule). */
  let { id, name, size = 8 }: { id: number; name?: string; size?: number } = $props();
  const n = $derived(((id - 1) % 4 + 4) % 4 + 1);
</script>

<span class="dot" style:--c="var(--repo-{n})" style:--s="{size}px" title={name} role={name ? 'img' : undefined} aria-label={name}></span>

<style>
  .dot {
    width: var(--s);
    height: var(--s);
    border-radius: 50%;
    background: var(--c);
    flex: none;
    display: inline-block;
  }
</style>
