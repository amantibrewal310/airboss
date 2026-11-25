<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { useApp } from '../../lib/state/context';
  import PlanCard from './PlanCard.svelte';

  /** Plans: a grid of cards at least 340px wide, 12px apart. ←→↑↓ move by card and row; ↩ opens. */
  const app = useApp();
  let grid: HTMLElement | undefined = $state();

  $effect(() => {
    const el = grid;
    if (!el) return;
    const measure = () => {
      app.planCols = Math.max(1, getComputedStyle(el).gridTemplateColumns.split(' ').filter(Boolean).length);
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  });

  // The selected card is the list's one Tab stop; arrow keys move real focus to it.
  $effect(() => {
    const i = app.ui.planIndex;
    void app.plans;
    const card = grid?.querySelectorAll<HTMLElement>('article.plan')[i];
    if (!card) return;
    card.scrollIntoView({ block: 'nearest' });
    app.focusTargets.list = card;
    if (grid?.contains(document.activeElement) && document.activeElement !== card) card.focus({ preventScroll: true });
  });
</script>

{#if app.plansLoaded && !app.plans.length}
  <div class="empty" bind:this={app.focusTargets.list} tabindex="-1">
    <Icon name="waves" size={22} />
    <h2>No plans yet</h2>
    <p>Plan it on any task turns its plan into subtasks here.</p>
    <Button variant="primary" onclick={() => app.openNew({ plan: true })}><Icon name="waves" size={13} />Plan something</Button>
  </div>
{:else}
  <div class="plans" bind:this={grid} role="list" aria-label="Plans">
    {#each app.plans as card, i (card.id)}
      <div class="cell" role="listitem">
        <PlanCard
          {card}
          repos={app.tasks.repoById}
          now={app.now}
          selected={i === app.ui.planIndex}
          onselect={() => (app.ui.planIndex = i)}
          onopen={() => app.openPage(card.id)}
          onnext={(id) => app.reveal(id)}
        />
      </div>
    {/each}
  </div>
{/if}

<style>
  .plans {
    flex: 1;
    overflow-y: auto;
    scrollbar-width: thin;
    padding: 18px 22px 40px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 340px), 1fr));
    gap: 12px;
    align-content: start;
    outline: none;
  }
  .cell {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .cell > :global(*) {
    flex: 1;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding-top: 160px;
    color: var(--text-2);
    gap: 4px;
    outline: none;
  }
  .empty h2 {
    font-size: var(--fs-title);
    font-weight: 600;
    color: var(--text);
    margin-top: 8px;
  }
  .empty p {
    margin-bottom: 14px;
  }
</style>
