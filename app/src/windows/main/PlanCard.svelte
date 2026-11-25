<script lang="ts">
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import ProgressRing from '../../lib/components/ProgressRing.svelte';
  import StackedBar from '../../lib/components/StackedBar.svelte';
  import RepoDot from '../../lib/components/RepoDot.svelte';
  import type { PlanCard, Repo, RepoId } from '../../lib/types';
  import { percent, shapeOf } from '../../lib/logic/views';
  import { relative } from '../../lib/logic/time';

  /**
   * A plan: status, title, id; the percentage at 26px with "7 of 20 done" and a 22px ring; the
   * stacked bar; then Wave (a dot per wave), Next up, Agents and Repos.
   */
  let {
    card,
    repos,
    now,
    selected = false,
    onselect,
    onopen,
    onnext,
  }: {
    card: PlanCard;
    repos: Map<RepoId, Repo>;
    now: number;
    selected?: boolean;
    onselect: () => void;
    onopen: () => void;
    onnext: (id: number) => void;
  } = $props();
  const pct = $derived(percent(card.progress));
  const summary = $derived(
    [
      `${pct}% done, ${card.progress.done} of ${card.progress.total}`,
      card.waves === 0 ? 'no waves yet' : card.current_wave === null ? 'all waves done' : `wave ${card.current_wave + 1} of ${card.waves}`,
      card.next_up.length ? `next up ${card.next_up.map((n) => `${n.id} ${n.title}`).join(', ')}` : card.finish_it ? 'finish it?' : 'nothing ready',
      card.working ? `${card.working} working` : '',
      card.blocked ? `${card.blocked} blocked` : '',
    ]
      .filter(Boolean)
      .join(', '),
  );
</script>

<!--
  A card is a focusable article (arrow keys move focus between cards; ↩ opens it). Its Next up
  pills are buttons inside it, so it isn't an option or a button itself.
-->
<!-- Keys come from the window's one key handler (arrows, ↩), so the article needs no keydown of its own. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<article
  class="plan"
  class:sel={selected}
  tabindex={selected ? 0 : -1}
  aria-labelledby="plan-title-{card.id}"
  aria-describedby="plan-sum-{card.id}"
  aria-current={selected ? 'true' : undefined}
  data-plan={card.id}
  onclick={onselect}
  ondblclick={onopen}
  onfocus={onselect}
>
  <span id="plan-sum-{card.id}" class="sr">{summary}</span>
  <h3><StatusIcon shape={shapeOf(card)} /><span class="title" id="plan-title-{card.id}" title={card.title}>{card.title}</span><span class="id" aria-hidden="true">{card.id}</span></h3>
  <div class="pctrow">
    <span class="pct">{pct}%</span>
    <span class="of">{card.progress.done} of {card.progress.total} done</span>
    <ProgressRing {pct} size={22} label="{card.progress.done} of {card.progress.total} done" />
  </div>
  <div class="barrow"><StackedBar p={card.progress} surface="var(--bg-raised)" /></div>
  <div class="rows">
    <div class="r">
      <span class="k">Wave</span>
      <span class="v">
        <!-- Only proposals so far: nothing is in a wave until one is accepted. -->
        {card.waves === 0 ? 'None yet, only proposals' : card.current_wave === null ? 'All waves done' : `${card.current_wave + 1} of ${card.waves}`}
        <span class="dots" aria-hidden="true">
          {#each { length: card.waves } as _, w (w)}
            <b class:d={card.current_wave === null || w < card.current_wave} class:c={w === card.current_wave}></b>
          {/each}
        </span>
      </span>
    </div>
    <div class="r">
      <span class="k">Next up</span>
      <span class="v">
        {#if card.next_up.length}
          {#each card.next_up as n (n.id)}
            <button type="button" class="pill ok" onclick={(e) => (e.stopPropagation(), onnext(n.id))}>{n.id} {n.title}</button>
          {/each}
        {:else if card.finish_it}
          <span class="pill ok">Finish it?</span>
        {:else}
          <span class="faint">Nothing ready</span>
        {/if}
      </span>
    </div>
    <div class="r">
      <span class="k">Agents</span>
      <span class="v">
        {#if card.working}
          <span class="live"><i></i>{card.working} working</span>
        {:else}
          <span class="faint">None running</span>
        {/if}
        {#if card.blocked}<span class="pill warn">{card.blocked} blocked</span>{/if}
      </span>
    </div>
    <div class="r">
      <span class="k">Repos</span>
      <span class="v repos">
        {#each card.repos as id (id)}
          <span class="repo"><RepoDot {id} />{repos.get(id)?.name}</span>
        {:else}
          <span class="faint">None</span>
        {/each}
      </span>
      <span class="faint when">{relative(card.last_at, now)}</span>
    </div>
  </div>
</article>

<style>
  .plan {
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    padding: 16px 16px 14px;
    background: var(--bg-raised);
    display: flex;
    flex-direction: column;
    outline: none;
    min-width: 0;
    transition:
      border-color 150ms var(--ease),
      transform 150ms var(--ease);
  }
  .plan:hover {
    border-color: var(--line-2);
    transform: translateY(-1px);
  }
  .plan.sel {
    border-color: var(--focus);
    box-shadow: 0 0 0 3px var(--focus-soft);
  }
  .plan:focus-visible {
    outline: none;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-card);
    font-weight: 600;
    letter-spacing: var(--track-title);
    line-height: 18px;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .id {
    font-size: var(--fs-meta);
    font-weight: 400;
    color: var(--text-3);
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
  }
  .pctrow {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 0 8px;
  }
  .pct {
    font-size: var(--fs-display);
    font-weight: 600;
    letter-spacing: var(--track-figure);
    line-height: 32px;
    font-variant-numeric: tabular-nums;
  }
  .of {
    flex: 1;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .barrow {
    display: flex;
  }
  .rows {
    padding-top: 12px;
    display: flex;
    flex-direction: column;
  }
  .r {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
  }
  .k {
    width: 62px;
    flex: none;
    color: var(--text-3);
  }
  .v {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 6px;
    align-items: center;
    overflow: hidden;
    white-space: nowrap;
  }
  .repos {
    gap: 14px;
  }
  .repo {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .when {
    font-variant-numeric: tabular-nums;
  }
  .faint {
    color: var(--text-3);
  }
  .dots {
    display: flex;
    gap: 4px;
    margin-left: 2px;
  }
  .dots b {
    width: 18px;
    height: 5px;
    border-radius: 3px;
    background: var(--wave-empty);
  }
  .dots b.d {
    background: var(--st-done);
  }
  .dots b.c {
    background: var(--st-doing);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    height: 22px;
    padding: 0 8px;
    border-radius: 11px;
    border: 1px solid var(--line-2);
    font-size: var(--fs-label);
    line-height: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .ok {
    color: var(--ok-text);
    border-color: color-mix(in srgb, var(--st-review) 35%, transparent);
  }
  button.ok:hover {
    background: color-mix(in srgb, var(--st-review) 10%, transparent);
  }
  .warn {
    color: var(--warn-text);
    border-color: color-mix(in srgb, var(--st-blocked) 35%, transparent);
    flex: none;
  }
  .live {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 20px;
    padding: 0 7px 0 6px;
    border-radius: 10px;
    font-size: var(--fs-label);
    color: var(--text-2);
    background: var(--bg-input);
    border: 1px solid var(--line);
    flex: none;
  }
  .live i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--st-review);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--st-review) 25%, transparent);
  }
</style>
