<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  /** 28px, 6px corners, 12.5px medium. One primary per view. */
  let {
    variant = 'secondary',
    size = 'md',
    children,
    ...rest
  }: { variant?: 'primary' | 'secondary' | 'ghost' | 'chip' | 'icon'; size?: 'md' | 'sm'; children: Snippet } & HTMLButtonAttributes = $props();
</script>

<button type="button" class="btn {variant} {size}" {...rest}>{@render children()}</button>

<style>
  .btn {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 0 11px;
    border-radius: var(--r-md);
    border: 1px solid transparent;
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    flex: none;
    transition:
      background var(--dur-hover) var(--ease),
      color var(--dur-hover) var(--ease),
      filter var(--dur-hover) var(--ease);
  }
  .primary {
    background: var(--accent-fill);
    color: var(--on-fill);
  }
  .primary:hover:not([aria-disabled='true']) {
    filter: brightness(1.1);
  }
  .secondary {
    background: var(--bg-input);
    border-color: var(--line-2);
    color: var(--text);
  }
  .secondary:hover:not([aria-disabled='true']) {
    background: var(--bg-sel);
  }
  .ghost {
    color: var(--text-2);
    padding: 0 10px;
  }
  .ghost:hover:not([aria-disabled='true']) {
    background: var(--bg-hover);
    color: var(--text);
  }
  .chip {
    height: 26px;
    padding: 0 9px;
    gap: 6px;
    border-color: var(--line-2);
    color: var(--text-2);
    font-size: var(--fs-meta);
    font-weight: 400;
  }
  .chip:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .icon {
    width: 26px;
    height: 26px;
    padding: 0;
    justify-content: center;
    color: var(--text-2);
  }
  .icon:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .sm {
    height: 24px;
    padding: 0 8px;
    font-size: var(--fs-meta);
  }
  .btn[aria-disabled='true'] {
    opacity: 0.45;
  }
  .btn:disabled {
    opacity: 0.45;
  }
</style>
