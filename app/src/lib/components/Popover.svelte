<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { outsideClick, trapTab } from './trap';

  /**
   * Floats at an anchor point, clamped to the window. Traps focus, closes on Esc and on a click
   * outside, and gives focus back (the caller restores it). Menus 9px corners; popovers 10px.
   */
  let {
    x,
    y,
    width,
    label,
    onclose,
    radius = 'var(--r-pop)',
    pad = '6px',
    role = 'dialog',
    autofocus = true,
    children,
  }: {
    x: number;
    y: number;
    width?: number;
    label: string;
    onclose: () => void;
    radius?: string;
    pad?: string;
    role?: 'dialog' | 'menu' | 'listbox' | 'none';
    autofocus?: boolean;
    children: Snippet;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  function place() {
    if (!el) return;
    const r = el.getBoundingClientRect();
    left = Math.max(8, Math.min(x, window.innerWidth - r.width - 8));
    top = y + r.height > window.innerHeight - 8 ? Math.max(8, y - r.height - 30) : y;
  }

  $effect(() => {
    void x;
    void y;
    place();
  });

  onMount(() => {
    place();
    if (autofocus && el && !el.contains(document.activeElement)) {
      const first =
        el.querySelector<HTMLElement>('[data-autofocus]') ??
        el.querySelector<HTMLElement>('input, textarea, [role="menuitem"], [role="option"], button');
      (first ?? el).focus({ preventScroll: true });
    }
    return outsideClick(() => el, onclose);
  });

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
      return;
    }
    if (el) trapTab(e, el);
  }
</script>

<div
  bind:this={el}
  class="pop"
  data-zone="overlay"
  style:left="{left}px"
  style:top="{top}px"
  style:width={width ? `${width}px` : undefined}
  style:border-radius={radius}
  style:padding={pad}
  role={role === 'none' ? undefined : role}
  aria-label={label}
  tabindex="-1"
  onkeydown={keydown}
>
  {@render children()}
</div>

<style>
  .pop {
    position: fixed;
    z-index: 70;
    background: var(--bg-raised);
    box-shadow: var(--shadow-menu);
    animation: drop var(--dur-menu) var(--ease);
    outline: none;
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
</style>
