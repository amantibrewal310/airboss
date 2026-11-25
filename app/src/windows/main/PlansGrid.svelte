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

  $effect(() => {
    const i = app.ui.planIndex;
    grid?.children[i]?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    app.focusTargets.list = grid;
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
  <div
    class="plans"
    bind:this={grid}
    role="listbox"
    tabindex="0"
    aria-label="Plans"
    aria-activedescendant={app.plans[app.ui.planIndex] ? `plan-${app.plans[app.ui.planIndex]!.id}` : undefined}
  >
    {#each app.plans as card, i (card.id)}
      <div id="plan-{card.id}" class="cell">
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
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 420px), 500px));
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
