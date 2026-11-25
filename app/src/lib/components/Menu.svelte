<script lang="ts" module>
  import type { Snippet } from 'svelte';

  export interface MenuItem {
    id: string;
    label: string;
    /** Rendered before the label (an icon or a status shape). */
    icon?: Snippet;
    /** Shortcut hint at the right ("⌘↩", "S"). */
    hint?: string;
    /** A reason means the item can't run; it shows greyed with the reason as its tooltip. */
    disabled?: string | false;
    checked?: boolean;
    /** Keeps the menu open after running (checklists). */
    stay?: boolean;
    run: () => void;
  }
  export type MenuEntry = MenuItem | 'separator';
</script>

<script lang="ts">
  import Popover from './Popover.svelte';

  /** Raised, 9px corners, 30px items with an icon, label and shortcut hint. ↑↓ move, ↩ picks, 1–9 pick by number when `numbered`. */
  let {
    items,
    x,
    y,
    width = 220,
    label,
    numbered = false,
    checklist = false,
    onclose,
    footer,
  }: {
    items: MenuEntry[];
    x: number;
    y: number;
    width?: number;
    label: string;
    numbered?: boolean;
    checklist?: boolean;
    onclose: () => void;
    footer?: Snippet;
  } = $props();

  const live = $derived(items.filter((i): i is MenuItem => i !== 'separator'));
  let hl = $state(0);
  /**
   * A menu opened by a key press must not be picked by that key's repeat: ↩, Space and clicks
   * wait 300 ms after opening, and ⌘↩ never picks. Number keys are an explicit choice and act at
   * once, so S then 4 typed quickly still works.
   */
  const SETTLE_MS = 300;
  const openedAt = performance.now();
  const settled = () => performance.now() - openedAt >= SETTLE_MS;
  let list: HTMLDivElement | undefined = $state();

  $effect(() => {
    const el = list?.querySelector<HTMLElement>(`[data-i="${hl}"]`);
    el?.focus({ preventScroll: true });
  });

  function pick(item: MenuItem | undefined, explicit = false) {
    if (!item || item.disabled) return;
    if (!explicit && !settled()) return;
    if (!item.stay) onclose();
    item.run();
  }

  function keydown(e: KeyboardEvent) {
    const n = live.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      e.stopPropagation();
      hl = (hl + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
    } else if (e.key === 'Enter' || (e.key === ' ' && checklist)) {
      e.preventDefault();
      e.stopPropagation();
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      pick(live[hl]);
    } else if (numbered && /^[1-9]$/.test(e.key) && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      e.stopPropagation();
      pick(live[Number(e.key) - 1], true);
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      hl = e.key === 'Home' ? 0 : n - 1;
    }
  }
</script>

<Popover {x} {y} {width} {label} {onclose} radius="var(--r-menu)" pad="5px" role="none">
  <div bind:this={list} class="menu" role="menu" aria-label={label} tabindex="-1" onkeydown={keydown}>
    {#each items as item, idx (item === 'separator' ? `sep${idx}` : item.id)}
      {#if item === 'separator'}
        <div class="sep" role="separator"></div>
      {:else}
        {@const i = live.indexOf(item)}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="item"
          class:hl={i === hl}
          class:off={!!item.disabled}
          role={checklist ? 'menuitemcheckbox' : 'menuitem'}
          aria-checked={checklist ? !!item.checked : undefined}
          aria-disabled={item.disabled ? 'true' : undefined}
          title={item.disabled || undefined}
          tabindex={i === hl ? 0 : -1}
          data-i={i}
          onpointermove={() => (hl = i)}
          onclick={() => pick(item)}
          onkeydown={() => {}}
        >
          {#if checklist}
            <span class="check" aria-hidden="true">
              {#if item.checked}<svg width="13" height="13" viewBox="0 0 16 16"><path d="m3.5 8.3 3 3 6-6.6" fill="none" stroke="var(--accent)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
            </span>
          {/if}
          {#if item.icon}<span class="ic">{@render item.icon()}</span>{/if}
          <span class="t">{item.label}</span>
          {#if item.hint}<span class="k">{item.hint}</span>{/if}
        </div>
      {/if}
    {/each}
  </div>
  {#if footer}{@render footer()}{/if}
</Popover>

<style>
  .menu {
    display: flex;
    flex-direction: column;
    outline: none;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 30px;
    padding: 0 9px;
    border-radius: var(--r-md);
    font-size: var(--fs-body);
    line-height: 16px;
    color: var(--text);
    outline: none;
    cursor: default;
  }
  .hl {
    background: var(--bg-sel);
  }
  .hl .k {
    color: var(--text-2);
  }
  .off {
    color: var(--text-3);
  }
  .ic {
    display: inline-flex;
    width: 16px;
    justify-content: center;
    color: var(--text-2);
    flex: none;
  }
  .check {
    width: 14px;
    display: inline-flex;
    justify-content: center;
    flex: none;
  }
  .t {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .k {
    color: var(--text-3);
    font-size: var(--fs-label);
    line-height: 14px;
    letter-spacing: 0;
  }
  .sep {
    height: 9px;
    display: flex;
    align-items: center;
    padding: 0 2px;
  }
  .sep::after {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--line);
  }
</style>
