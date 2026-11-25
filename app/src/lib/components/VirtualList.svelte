<script lang="ts" generics="T extends { key: string }">
  import type { Snippet } from 'svelte';
  import { indexAt, prefixSums, scrollToShow, visibleRange } from '../logic/virtual';

  /**
   * Our own virtual list: fixed heights, items positioned with translateY in a `contain: strict`
   * scroller, keyed, 8 rows of overscan. The active item is always rendered (for
   * aria-activedescendant) and kept in view with "nearest" scrolling. An optional sticky header
   * mirrors the group the top row belongs to.
   */
  let {
    items,
    heightOf,
    active = -1,
    overscan = 8,
    padTop = 6,
    padBottom = 30,
    stickyOf,
    stickyHeight = 34,
    item,
    sticky,
    scroller = $bindable(),
    ...rest
  }: {
    items: T[];
    heightOf: (item: T) => number;
    active?: number;
    overscan?: number;
    padTop?: number;
    padBottom?: number;
    /** The group item (index) the item at `index` belongs to, for the sticky header. */
    stickyOf?: (index: number) => number;
    stickyHeight?: number;
    item: Snippet<[T, number]>;
    sticky?: Snippet<[T, number]>;
    scroller?: HTMLElement;
    [attr: string]: unknown;
  } = $props();

  let scrollTop = $state(0);
  let viewport = $state(800);

  const offsets = $derived(prefixSums(items.map(heightOf), padTop));
  const total = $derived(offsets[items.length]! + padBottom);
  const range = $derived(visibleRange(offsets, scrollTop, viewport, overscan));
  const shown = $derived.by(() => {
    const out: number[] = [];
    for (let i = range.start; i < range.end; i++) out.push(i);
    if (active >= 0 && active < items.length && (active < range.start || active >= range.end)) out.push(active);
    return out;
  });

  const stickyIndex = $derived(stickyOf && items.length ? stickyOf(indexAt(offsets, scrollTop)) : -1);
  const stickyShift = $derived.by(() => {
    if (stickyIndex < 0 || !stickyOf) return 0;
    // The next group's header pushes the sticky one up as it arrives.
    for (let i = stickyIndex + 1; i < items.length && offsets[i]! - scrollTop <= stickyHeight; i++) {
      if (stickyOf(i) === i) return offsets[i]! - scrollTop - stickyHeight;
    }
    return 0;
  });

  $effect(() => {
    const el = scroller;
    if (!el) return;
    const ro = new ResizeObserver(() => (viewport = el.clientHeight));
    ro.observe(el);
    viewport = el.clientHeight;
    return () => ro.disconnect();
  });

  // Keep the active item in view, without smooth scrolling.
  $effect(() => {
    const el = scroller;
    if (!el || active < 0) return;
    const cover = stickyOf && active > 0 && stickyOf(active) !== active ? stickyHeight : 0;
    const next = scrollToShow(offsets, active, el.scrollTop, el.clientHeight, cover, padBottom);
    if (next !== null) {
      el.scrollTop = next;
      scrollTop = el.scrollTop;
    }
  });

  // Read the position on the scroll event itself, so a fast fling never shows blank space, and
  // once more on the next frame to catch where momentum scrolling settles.
  let frame = 0;
  function onscroll() {
    if (scroller) scrollTop = scroller.scrollTop;
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (scroller) scrollTop = scroller.scrollTop;
    });
  }
</script>

<div class="vl">
<div class="scroller" bind:this={scroller} {onscroll} {...rest}>
  <div class="space" style:height="{total}px">
    {#each shown as i (items[i]!.key)}
      <div class="slot" style:transform="translateY({offsets[i]}px)" style:height="{heightOf(items[i]!)}px">
        {@render item(items[i]!, i)}
      </div>
    {/each}
  </div>
</div>
{#if sticky && stickyIndex >= 0 && scrollTop > offsets[stickyIndex]! - 1}
  <div class="sticky" style:transform="translateY({stickyShift}px)" style:height="{stickyHeight}px" aria-hidden="true">
    {@render sticky(items[stickyIndex]!, stickyIndex)}
  </div>
{/if}
</div>

<style>
  .vl {
    position: relative;
    height: 100%;
    min-height: 0;
  }
  .scroller {
    position: relative;
    overflow-y: auto;
    overflow-x: hidden;
    contain: strict;
    height: 100%;
    outline: none;
    scrollbar-width: thin;
    overscroll-behavior: contain;
  }
  .space {
    position: relative;
    width: 100%;
  }
  .slot {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    will-change: transform;
  }
  .sticky {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 2;
    pointer-events: none;
  }
</style>
