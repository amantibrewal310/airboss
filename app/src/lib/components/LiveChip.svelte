<script lang="ts">
  import type { Chip } from '../types';

  /**
   * A thread's live state after the title: Agent working (pulsing green), Waiting on you (red),
   * Stopped (grey). `short` uses the Threads labels; `dotOnly` is for narrow widths (the label
   * stays as the accessible name and tooltip).
   */
  let { chip, short = false, dotOnly = false, text }: { chip: Chip; short?: boolean; dotOnly?: boolean; text?: string } = $props();
  const label = $derived(
    text ?? (short ? { working: 'Running', waiting_on_you: 'Waiting', stopped: 'Stopped' } : { working: 'Agent working', waiting_on_you: 'Waiting on you', stopped: 'Stopped' })[chip],
  );
</script>

<span class="live {chip}" class:dot-only={dotOnly} title={label} role="img" aria-label={label}>
  <i></i>{#if !dotOnly}<span class="lt">{label}</span>{/if}
</span>

<style>
  .live {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 20px;
    padding: 0 7px 0 6px;
    border-radius: 10px;
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-2);
    background: var(--bg-input);
    border: 1px solid var(--line);
    white-space: nowrap;
    flex: none;
  }
  .dot-only {
    padding: 0 6px;
  }
  i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    position: relative;
    flex: none;
    background: var(--text-3);
  }
  .working i {
    background: var(--st-review);
  }
  .waiting_on_you i {
    background: var(--needs);
  }
  .working i::after {
    content: '';
    position: absolute;
    inset: -3px;
    border-radius: 50%;
    border: 1.5px solid var(--st-review);
    animation: pulse var(--dur-pulse) ease-out infinite;
    opacity: 0;
  }
  @keyframes pulse {
    from {
      opacity: 0.9;
      transform: scale(0.6);
    }
    to {
      opacity: 0;
      transform: scale(1.8);
    }
  }
  /* Reduce Motion: the pulse becomes a static ring. */
  @media (prefers-reduced-motion: reduce) {
    .working i::after {
      opacity: 0.35;
      transform: scale(1.4);
    }
  }
</style>
