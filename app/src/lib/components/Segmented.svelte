<script lang="ts" generics="T extends string">
  /** Switches a view, never submits. Track bg-input; the active segment is selected with a ring. */
  let {
    options,
    value,
    onchange,
    label,
  }: { options: { value: T; label: string }[]; value: T; onchange: (v: T) => void; label: string } = $props();

  function key(e: KeyboardEvent) {
    const i = options.findIndex((o) => o.value === value);
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      e.stopPropagation();
      const next = options[(i + (e.key === 'ArrowRight' ? 1 : -1) + options.length) % options.length]!;
      onchange(next.value);
    }
  }
</script>

<div class="seg" role="radiogroup" aria-label={label} tabindex="-1" onkeydown={key}>
  {#each options as o (o.value)}
    <button
      type="button"
      role="radio"
      aria-checked={o.value === value}
      tabindex={o.value === value ? 0 : -1}
      class:on={o.value === value}
      onclick={() => onchange(o.value)}>{o.label}</button
    >
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--r-md);
    background: var(--bg-input);
    border: 1px solid var(--line);
    flex: none;
  }
  button {
    padding: 1px 9px;
    border-radius: var(--r-xs);
    color: var(--text-2);
    font-size: var(--fs-meta);
    line-height: 16px;
  }
  button:hover {
    color: var(--text);
  }
  .on {
    background: var(--seg-on);
    color: var(--text);
    /* In light mode the selected segment is raised white. */
    box-shadow:
      0 0 0 1px var(--line-2),
      var(--seg-lift, 0 0 transparent);
  }
</style>
