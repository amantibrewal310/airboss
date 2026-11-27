<script lang="ts">
  import { fullTitle } from '../../../lib/actions/fullTitle';
  import Icon from '../../../lib/components/Icon.svelte';
  import StatusIcon from '../../../lib/components/StatusIcon.svelte';
  import type { ProposalDto, RunMap, TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { headlineOf, KIND_ICON, mainLabel } from '../../../lib/logic/needs';
  import { needsUnder, planSections, planSummary, proposalEdges, proposalTurns, subtreeOf } from '../../../lib/logic/plan';
  import StepRows from './StepRows.svelte';

  /**
   * A parent's steps as decisions (task 54): a summary line, Needs you (each item with its own
   * action, proposals one by one), Ready now with Start ready, In progress, Next, and Done folded.
   * Run order sorts every list; the word for its columns stays out of the UI.
   */
  let {
    task,
    map,
    proposals,
    summary = true,
    asks: showAsks = true,
    steps = true,
  }: {
    task: TaskId;
    map: RunMap | null;
    proposals: ProposalDto[] | null;
    /** The summary line (the page shows its progress bar instead). */
    summary?: boolean;
    /** Needs you (the page shows it above the notes). */
    asks?: boolean;
    /** Ready now, In progress, Next and Done (the page shows them under its List tab). */
    steps?: boolean;
  } = $props();
  const app = useApp();
  const uid = $props.id();

  const row = $derived(app.row(task));
  const inside = $derived(subtreeOf(app.tasks.rows, app.tasks.order, task));
  const sections = $derived(planSections(app.tasks.rows, app.tasks.order, task, map));
  const needs = $derived(needsUnder(app.needs.visible, inside));
  const outside = $derived(new Map((map?.outside ?? []).map((o) => [o.task, o.blockers])));

  /** Open proposals: the detail's, plus any the Needs me list knows that it doesn't yet. */
  const turns = $derived.by(() => {
    const open = (id: TaskId) => app.row(id)?.status === 'proposed' && !app.tasks.hidden.has(id);
    const list = (proposals ?? []).filter((p) => open(p.id));
    const known = new Set(list.map((p) => p.id));
    for (const n of app.needs.visible) {
      if (n.kind !== 'proposed' || !inside.has(n.task)) continue;
      for (const [id] of n.proposed) {
        if (known.has(id) || !open(id)) continue;
        known.add(id);
        list.push({ id, parent: app.row(id)?.parent ?? n.task, by: '', at: '', follows: [], blocks: [] });
      }
    }
    return proposalTurns(list);
  });
  const asks = $derived(needs.length + turns.reduce((n, t) => n + t.length, 0));
  let showDone = $state(false);
</script>

{#if summary && row?.progress}
  <p class="sum">{planSummary(row.progress, sections.ready.length)}</p>
{/if}

{#if showAsks && asks}
  <section class="part" aria-labelledby="needs-{uid}">
    <h3 id="needs-{uid}">Needs you <span class="n">{asks}</span></h3>
    {#each needs as n (n.key)}
      <div class="ask" class:stale={n.stale} class:gone={app.needs.leaving.has(n.key)}>
        <span class="ic"><Icon name={KIND_ICON[n.kind]} size={15} /></span>
        <button type="button" class="what" onclick={() => app.reveal(n.task)}>
          <span class="id">{n.task}</span><span class="t" use:fullTitle={headlineOf(n)}>{headlineOf(n)}</span>
        </button>
        <button type="button" class="act sec" onclick={() => app.mainAction(n)}>{mainLabel(n.kind)}</button>
      </div>
    {/each}
    {#each turns as turn (turn[0]!.id)}
      <div class="turn" class:many={turn.length > 1}>
        {#if turn.length > 1}
          <div class="turn-h">
            <span>{turn.length} proposed together</span>
            <button type="button" class="act sec" onclick={() => app.acceptTurn(turn.map((p) => p.id))}>Accept all</button>
          </div>
        {/if}
        {#each turn as p (p.id)}
          {@const r = app.row(p.id)}
          {@const edges = proposalEdges(p)}
          <div class="ask prop">
            <span class="ic plain"><StatusIcon shape="proposed" label={false} /></span>
            <button type="button" class="what" onclick={() => app.reveal(p.id)}>
              <span class="id">{p.id}</span>
              <span class="tw">
                <span class="t" use:fullTitle={r?.title ?? ''}>{r?.title}</span>
                {#if edges}<small>{edges}</small>{/if}
              </span>
            </button>
            <button type="button" class="act sec" onclick={() => app.acceptOne(p.id)}>Accept</button>
            <button type="button" class="act sec" onclick={() => app.rejectOne(p.id)}>Reject</button>
          </div>
        {/each}
      </div>
    {/each}
  </section>
{/if}

{#if steps && sections.ready.length}
  <section class="part" aria-labelledby="ready-{uid}">
    <h3 id="ready-{uid}" class="with-act">
      <span>Ready now <span class="n">{sections.ready.length}</span></span>
      <button
        type="button"
        class="act sec"
        aria-disabled={!app.conn.launchReady || row?.waiting || app.ui.starting.has(task) ? 'true' : undefined}
        title={app.conn.launchReady ? `An agent starts on each of the ${sections.ready.length} ready step${sections.ready.length === 1 ? '' : 's'}` : app.conn.launchOff}
        onclick={() => app.startReady(task)}
      ><Icon name="play" size={13} />Start ready</button>
    </h3>
    <StepRows ids={sections.ready} {outside} />
  </section>
{/if}

{#if steps && sections.doing.length}
  <section class="part" aria-labelledby="doing-{uid}">
    <h3 id="doing-{uid}">In progress <span class="n">{sections.doing.length}</span></h3>
    <StepRows ids={sections.doing} {outside} />
  </section>
{/if}

{#if steps && sections.next.length}
  <section class="part" aria-labelledby="next-{uid}">
    <h3 id="next-{uid}">Next <span class="n">{sections.next.length}</span></h3>
    <StepRows ids={sections.next} {outside} />
  </section>
{/if}

{#if steps && sections.done.length}
  <section class="part" aria-label="Done">
    <h3>
      <button type="button" class="fold" aria-expanded={showDone} aria-controls="done-{uid}" onclick={() => (showDone = !showDone)}>
        <span class="chev" class:open={showDone}><Icon name="chevron" size={12} /></span>Done <span class="n">{sections.done.length}</span>
      </button>
    </h3>
    <div id="done-{uid}" hidden={!showDone}>
      {#if showDone}<StepRows ids={sections.done} />{/if}
    </div>
  </section>
{/if}

{#if steps && map?.tangled.length}
  <p class="tangled">These wait on each other in a loop: {map.tangled.join(', ')}.</p>
{/if}

<style>
  .sum {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    line-height: 19px;
  }
  .part {
    display: flex;
    flex-direction: column;
  }
  h3 {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
    margin-bottom: 4px;
  }
  .n {
    color: var(--text-3);
    font-weight: 500;
    margin-left: 4px;
  }
  h3.with-act {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .fold {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font: inherit;
    color: inherit;
    margin-left: -4px;
    padding: 2px 4px;
    border-radius: var(--r-md);
  }
  .fold:hover {
    color: var(--text);
  }
  /* The chevron points down; folded, it turns to point right. */
  .chev {
    display: inline-grid;
    transform: rotate(-90deg);
    transition: transform var(--dur-panel) var(--ease);
  }
  .chev.open {
    transform: none;
  }
  .ask {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 36px;
    padding: 4px 0;
    transition:
      opacity var(--dur-exit) var(--ease),
      transform var(--dur-exit) var(--ease);
  }
  .gone {
    opacity: 0;
    transform: translateX(-10px);
  }
  .stale .t {
    color: var(--text-2);
  }
  .ic {
    color: var(--needs);
    flex: none;
    display: grid;
    width: 16px;
  }
  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 9px;
    text-align: left;
    font-size: var(--fs-body);
    line-height: 16px;
  }
  .what:hover .t {
    color: var(--text);
    text-decoration: underline;
    text-decoration-color: var(--line-2);
  }
  .id {
    width: 30px;
    flex: none;
    color: var(--text-3);
    font-size: var(--fs-meta);
    font-variant-numeric: tabular-nums;
  }
  .tw {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small {
    font-size: var(--fs-label);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .turn.many {
    border-left: 2px solid var(--line-2);
    padding-left: 10px;
    margin: 4px 0 4px 6px;
  }
  .turn-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--fs-label);
    color: var(--text-3);
    min-height: 28px;
  }
  .act {
    height: 26px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    border-radius: var(--r-md);
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    flex: none;
    border: 1px solid transparent;
  }
  .act.sec {
    background: var(--bg-input);
    border-color: var(--line-2);
    color: var(--text);
  }
  .act.sec:hover:not([aria-disabled='true']) {
    background: var(--bg-sel);
  }
  [aria-disabled='true'] {
    opacity: 0.45;
  }
  .tangled {
    font-size: var(--fs-meta);
    color: var(--warn-text);
  }
</style>
