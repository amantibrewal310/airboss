<script lang="ts">
  import { SHAPE_LABEL, type Shape } from '../logic/views';

  /** 16px, drawn on a 6.25px ring with a 1.6px stroke. Shape carries the meaning before colour. */
  let { shape, size = 16, pop = 0, label = true }: { shape: Shape; size?: number; pop?: number; label?: boolean } = $props();
</script>

{#key pop}
  <svg
    class="st"
    class:pop={pop > 0}
    width={size}
    height={size}
    viewBox="0 0 16 16"
    role={label ? 'img' : undefined}
    aria-label={label ? SHAPE_LABEL[shape] : undefined}
    aria-hidden={label ? undefined : 'true'}
  >
    {#if shape === 'done'}
      <circle cx="8" cy="8" r="7" fill="var(--st-done)" />
      <path d="M5 8.2l2 2 4-4.3" fill="none" stroke="var(--on-fill)" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
    {:else}
      <circle
        cx="8"
        cy="8"
        r="6.25"
        fill="none"
        stroke={shape === 'doing' ? 'var(--st-doing)' : shape === 'in_review' ? 'var(--st-review)' : shape === 'blocked' ? 'var(--st-blocked)' : 'var(--st-todo)'}
        stroke-width="1.6"
        stroke-dasharray={shape === 'waiting' ? '2.4 2.2' : shape === 'proposed' ? '1.2 2.2' : undefined}
      />
      {#if shape === 'doing'}
        <path d="M8 8V4.2a3.8 3.8 0 0 1 0 7.6Z" fill="var(--st-doing)" />
      {:else if shape === 'in_review'}
        <path d="M8 8V4.2a3.8 3.8 0 1 1-3.8 3.8Z" fill="var(--st-review)" />
      {:else if shape === 'blocked'}
        <path d="M5.5 8h5" stroke="var(--st-blocked)" stroke-width="2" stroke-linecap="round" />
      {:else if shape === 'dropped'}
        <path d="M5.6 5.6l4.8 4.8M10.4 5.6l-4.8 4.8" stroke="var(--st-todo)" stroke-width="1.4" stroke-linecap="round" />
      {/if}
    {/if}
  </svg>
{/key}

<style>
  .st {
    flex: none;
    display: block;
  }
  .pop {
    animation: pop var(--dur-pop) var(--ease);
  }
  @keyframes pop {
    0% {
      transform: scale(0.5);
    }
    60% {
      transform: scale(1.18);
    }
    100% {
      transform: scale(1);
    }
  }
</style>
