<script lang="ts">
  import Icon from '../../lib/components/Icon.svelte';
  import { useApp } from '../../lib/state/context';

  /**
   * Bottom right, raised, with an indigo spark (an orange warning icon for errors; red is for
   * needs-me only). They rise 6px in; with an action they stay 5 s and pause while hovered.
   * F6 moves focus to the newest toast and back.
   */
  const app = useApp();
</script>

<div class="toasts" role="status" aria-live="polite" aria-label="Notifications">
  {#each app.toasts.list as t (t.id)}
    <div
      class="toast"
      class:warn={t.tone === 'warn'}
      data-toast
      role="group"
      aria-label={t.text}
      onpointerenter={() => app.toasts.pause(t.id)}
      onpointerleave={() => app.toasts.resume(t.id)}
      onfocusin={() => app.toasts.pause(t.id)}
      onfocusout={() => app.toasts.resume(t.id)}
    >
      <span class="ic">{#if t.tone === 'warn'}<Icon name="warn" size={14} />{:else}<Icon name="spark" size={14} />{/if}</span>
      <span class="tx">{t.text}{#if t.detail}<small>{t.detail}</small>{/if}</span>
      {#each t.actions as a (a.label)}
        <button
          type="button"
          class="act"
          onclick={() => {
            app.toasts.dismiss(t.id);
            a.run();
          }}>{a.label}</button
        >
      {/each}
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
</style>
