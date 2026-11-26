<script lang="ts">
  import Icon from '../../lib/components/Icon.svelte';
  import { useApp } from '../../lib/state/context';

  /**
   * Bottom right, raised, with an indigo spark (an orange warning icon for errors; red is for
   * needs-me only). They rise 6px in; with an action they stay 5 s and pause while hovered.
   * An error whose fix is its action stays, with a close button, until it's used or closed.
   * F6 moves focus to the newest toast and back; Esc in a toast closes it and puts focus back.
   */
  const app = useApp();
  let root: HTMLDivElement | undefined = $state();
  /** Where focus was before it came into the toasts. */
  let back: HTMLElement | null = null;

  function focusIn(e: FocusEvent, id: number) {
    app.toasts.pause(id);
    const from = e.relatedTarget;
    if (from instanceof HTMLElement && !root?.contains(from)) back = from;
  }

  /** Closes a toast; if it had focus, focus goes back where it came from. */
  function close(id: number) {
    const had = !!root?.contains(document.activeElement);
    app.toasts.dismiss(id);
    if (!had) return;
    const to = back;
    back = null;
    if (to?.isConnected) to.focus({ preventScroll: true });
    else app.focus('list');
  }

  function key(e: KeyboardEvent, id: number) {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    close(id);
  }
</script>

<div class="toasts" role="status" aria-live="polite" aria-label="Notifications" bind:this={root}>
  {#each app.toasts.list as t (t.id)}
    <div
      class="toast"
      class:warn={t.tone === 'warn'}
      data-toast
      role="group"
      aria-label={t.text}
      onpointerenter={() => app.toasts.pause(t.id)}
      onpointerleave={() => app.toasts.resume(t.id)}
      onfocusin={(e) => focusIn(e, t.id)}
      onfocusout={() => app.toasts.resume(t.id)}
    >
      <span class="ic">{#if t.tone === 'warn'}<Icon name="warn" size={14} />{:else}<Icon name="spark" size={14} />{/if}</span>
      <span class="tx">{t.text}{#if t.detail}<small>{t.detail}</small>{/if}</span>
      {#each t.actions as a (a.label)}
        <button
          type="button"
          class="act"
          onkeydown={(e) => key(e, t.id)}
          onclick={() => {
            close(t.id);
            a.run();
          }}>{a.label}</button
        >
      {/each}
      {#if t.sticky}
        <button type="button" class="x" aria-label="Close" title="Close" onkeydown={(e) => key(e, t.id)} onclick={() => close(t.id)}><Icon name="close" size={13} /></button>
      {/if}
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 18px;
    bottom: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-end;
    z-index: 80;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 9px;
    max-width: 440px;
    padding: 9px 13px;
    border-radius: var(--r-lg);
    background: var(--bg-raised);
    border: 1px solid var(--line-2);
    box-shadow: var(--shadow-float);
    font-size: var(--fs-control);
    line-height: 17px;
    animation: rise var(--dur-toast) var(--ease);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .ic {
    color: var(--accent);
    display: flex;
    flex: none;
    align-self: flex-start;
    padding-top: 1px;
  }
  .warn .ic {
    color: var(--st-blocked);
  }
  .tx {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  small {
    display: block;
    color: var(--text-2);
    font-size: var(--fs-meta);
    margin-top: 2px;
  }
  .act {
    flex: none;
    color: var(--accent);
    font-weight: 500;
    padding: 2px 6px;
    margin: -2px -4px -2px 0;
    border-radius: var(--r-sm);
    white-space: nowrap;
  }
  .act:hover {
    background: var(--bg-hover);
  }
  .x {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin: -3px -7px -3px -2px;
    border-radius: var(--r-sm);
    color: var(--text-2);
  }
  .x:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
</style>
