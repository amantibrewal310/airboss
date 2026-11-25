<script lang="ts">
  import type { Snippet } from 'svelte';

  /** 22px, fully rounded, hairline border, label type. Colour only when it means something. */
  let {
    tone = 'neutral',
    title,
    children,
  }: { tone?: 'neutral' | 'warn' | 'ok' | 'need'; title?: string; children: Snippet } = $props();
</script>

<span class="pill {tone}" {title}>{@render children()}</span>

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 8px;
    border-radius: 11px;
    border: 1px solid var(--line-2);
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-2);
    white-space: nowrap;
    flex: none;
    max-width: 100%;
  }
  .warn {
    color: var(--st-blocked);
    border-color: color-mix(in srgb, var(--st-blocked) 35%, transparent);
  }
  .ok {
    color: var(--st-review);
    border-color: color-mix(in srgb, var(--st-review) 35%, transparent);
  }
  .need {
    color: var(--needs);
    border-color: color-mix(in srgb, var(--needs) 35%, transparent);
    background: var(--needs-soft);
  }
</style>
