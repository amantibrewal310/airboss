<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { trapTab } from './trap';

  /** A modal on the scrim (black 35%, 12% in light): fades, drops 4px and scales from 98.5%. */
  let {
    label,
    top = '16%',
    width = 680,
    onclose,
    children,
  }: { label: string; top?: string; width?: number; onclose: () => void; children: Snippet } = $props();

  let el: HTMLDivElement | undefined = $state();

  onMount(() => {
    if (el && !el.contains(document.activeElement)) {
      const first = el.querySelector<HTMLElement>('[data-autofocus], input, textarea, button');
      (first ?? el).focus({ preventScroll: true });
    }
  });

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !e.defaultPrevented) {
      e.preventDefault();
      e.stopPropagation();
      onclose();
      return;
    }
    if (el) trapTab(e, el);
  }
</script>

<div class="scrim" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div
    bind:this={el}
    class="modal"
    style:top
    style:width="min({width}px, calc(100% - 32px))"
    role="dialog"
    aria-modal="true"
    aria-label={label}
    tabindex="-1"
    onkeydown={keydown}
  >
    {@render children()}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--scrim);
    animation: fade var(--dur-modal) var(--ease);
  }
  .modal {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-raised);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-float);
    overflow: hidden;
    outline: none;
    animation: rise var(--dur-modal) var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-4px) scale(0.985);
    }
  }
</style>
