<script lang="ts">
  import { fullTitle } from '../../lib/actions/fullTitle';
  import type { Repo, RepoId, RowDto } from '../../lib/types';
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import ProgressRing from '../../lib/components/ProgressRing.svelte';
  import RepoDot from '../../lib/components/RepoDot.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { percent, shapeOf, signalLabel, SHAPE_LABEL } from '../../lib/logic/views';
  import { relative, relativeLong } from '../../lib/logic/time';

  /**
   * One task row: exactly 38px, nothing wraps. Left to right: fold arrow, status, id, title,
   * priority badge, progress (a parent), at most one chip (`row.signal`, task 55), gap, repo
   * pills (a leaf), time. No effects and no listeners: the list handles clicks by delegation
   * through the data-* attributes.
   */
  let {
    row,
    depth,
    repos,
    now,
    selected = false,
    keyboard = false,
    folded = false,
    foldable = true,
    fresh = false,
    pop = 0,
    renaming = false,
    since = false,
    role = 'option',
    pos = 0,
    size = 0,
    onrename,
  }: {
    row: RowDto;
    depth: number;
    repos: Map<RepoId, Repo>;
    now: number;
    selected?: boolean;
    keyboard?: boolean;
    folded?: boolean;
    foldable?: boolean;
    fresh?: boolean;
    pop?: number;
    renaming?: boolean;
    /** Ready view: the time column shows when the task became ready. */
    since?: boolean;
    role?: 'treeitem' | 'option';
    pos?: number;
    size?: number;
    onrename?: (title: string | null) => void;
  } = $props();

  const shape = $derived(shapeOf(row));
  // A parent's repos are its steps' repos, the same on every phase: only leaves show them.
  const names = $derived(row.has_children ? [] : row.repos.map((id) => repos.get(id)).filter((r): r is Repo => !!r));
  const shownRepos = $derived(names.slice(0, 2));
  const pct = $derived(percent(row.progress));
  const time = $derived(since ? row.ready_since : row.last_at);
  const sig = $derived(row.signal);
  const sigLabel = $derived(sig ? signalLabel(sig, row) : '');
  const stale = $derived(sig && 'stale' in sig && sig.stale);
  const aria = $derived(
    [
      row.id,
      row.title,
      SHAPE_LABEL[shape],
      row.priority ? `priority ${row.priority.toUpperCase()}` : '',
      row.status === 'blocked' && row.blocked_reason ? `blocked: ${row.blocked_reason}` : '',
      row.progress ? `${row.progress.done} of ${row.progress.total} done` : '',
      sigLabel ? (stale ? `${sigLabel} (may be out of date)` : sigLabel) : '',
      names.map((r) => r.name).join(', '),
      relativeLong(time, now),
    ]
      .filter(Boolean)
      .join(', '),
  );

  let input: HTMLInputElement | undefined = $state();
  $effect(() => {
    if (renaming && input) {
      input.focus();
      input.select();
    }
  });

  function renameKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      onrename?.(input?.value ?? null);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      onrename?.(null);
    }
  }
</script>

<div
  id="row-{row.id}"
  class="row"
  class:sel={selected}
  class:kb={selected && keyboard}
  class:done={row.status === 'done'}
  class:fresh
  style:--d={depth}
  data-row={row.id}
  {role}
  aria-selected={selected}
  aria-level={role === 'treeitem' ? depth + 1 : undefined}
  aria-expanded={role === 'treeitem' && row.has_children ? !folded : undefined}
  aria-setsize={role === 'treeitem' ? size : undefined}
  aria-posinset={role === 'treeitem' ? pos : undefined}
>
  <!-- The accessible name is this sentence; the terse visible pieces below are hidden from
       VoiceOver so nothing is read twice. Voice Control still matches on the title. -->
  <span class="sr">{aria}</span>
  {#if foldable && row.has_children}
    <span class="disc" class:open={!folded} data-fold={row.id} aria-hidden="true"><Icon name="chevron" size={10} /></span>
  {:else}
    <span class="disc" aria-hidden="true"></span>
  {/if}
  <span class="stbtn" data-status-for={row.id} title="Change status (S)"><StatusIcon {shape} {pop} label={false} /></span>
  <span class="id" aria-hidden="true">{row.id}</span>
  {#if renaming}
    <input bind:this={input} class="rename" value={row.title} aria-label="Rename {row.id}" onkeydown={renameKey} onblur={() => onrename?.(input?.value ?? null)} />
  {:else}
    <span class="t" use:fullTitle={row.title} aria-hidden="true">{row.title}</span>
  {/if}
  {#if row.priority}
    <span class="pri {row.priority}" title="Priority {row.priority.toUpperCase()}" aria-hidden="true">{row.priority.toUpperCase()}</span>
  {/if}
  {#if row.progress}
    <span class="prog" title="{pct}% done" aria-hidden="true">
      <ProgressRing {pct} size={13} /><span class="pc">{row.progress.done}/{row.progress.total}</span>
    </span>
  {/if}
  {#if sig}
    <span class="sig {sig.kind}" class:stale title={stale ? `${sigLabel} · may be out of date` : sigLabel} aria-hidden="true">
      {#if sig.kind === 'needs_you'}
        <Icon name="inbox" size={12} /><span class="lab">{sigLabel}</span><span class="n">{sig.count}</span>
      {:else if sig.kind === 'failed'}
        <Icon name="failed" size={12} /><span class="lab">Failed</span>
      {:else if sig.kind === 'pr_open'}
        <Icon name="pr" size={12} /><span class="lab">PR open</span>
      {:else}
        <Icon name="dep" size={12} /><span class="long">{sigLabel}</span><span class="short">{row.waits_on.length}</span>
      {/if}
    </span>
  {/if}
  <span class="sp"></span>
  {#if names.length}
    <span class="meta" aria-hidden="true">
      {#each shownRepos as r (r.id)}
        <span class="pill rn" title={r.name}><RepoDot id={r.id} />{r.name}</span>
      {/each}
      {#if names.length > 2}<span class="pill rn" title={names.slice(2).map((r) => r.name).join(', ')}>+{names.length - 2}</span>{/if}
    </span>
    <span class="rd" aria-hidden="true">
      {#each names as r (r.id)}<RepoDot id={r.id} name={r.name} />{/each}
    </span>
  {/if}
  <span class="when" aria-hidden="true">{relative(time, now)}</span>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--row-h);
    padding: 0 16px 0 calc(14px + var(--d, 0) * var(--indent));
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
    position: relative;
    line-height: 16px;
    overflow: hidden;
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.sel {
    background: var(--bg-sel);
  }
  /* On the selection wash, ids, fractions and times step up to text-2 to keep 4.5:1. */
  .row.sel .id,
  .row.sel .prog,
  .row.sel .when {
    color: var(--text-2);
  }
  .row.kb::after {
    content: '';
    position: absolute;
    inset: 0;
    box-shadow: inset 0 0 0 var(--kb-outline) var(--focus);
    pointer-events: none;
  }
  .row.fresh {
    animation: fresh var(--dur-fresh) var(--ease);
  }
  @keyframes fresh {
    from {
      background: var(--accent-soft);
    }
  }
  .disc {
    width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
    color: var(--text-3);
    flex: none;
  }
  .disc :global(svg) {
    transform: rotate(-90deg);
    transition: transform 150ms var(--ease);
  }
  .disc.open :global(svg) {
    transform: none;
  }
  .disc[data-fold]:hover {
    color: var(--text);
  }
  .stbtn {
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    flex: none;
  }
  .stbtn:hover {
    background: var(--bg-hover);
    box-shadow: 0 0 0 3px var(--bg-hover);
  }
  .id {
    font-variant-numeric: tabular-nums;
    width: 34px;
    flex: none;
    color: var(--text-3);
    font-size: var(--fs-meta);
  }
  .t {
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
    flex: 0 1 auto;
    min-width: 48px;
  }
  .done .t {
    color: var(--text-2);
  }
  .rename {
    flex: 1;
    min-width: 0;
    height: 26px;
    margin: 0 -7px;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    padding: 0 6px;
    outline: none;
    font-weight: 500;
  }
  /* A tag, not a pill: Field Grey, 5px corners. P0 and P1 read in full text colour; red stays
     for "needs you" only (DESIGN), so urgency is weight, not colour. */
  .pri {
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-label);
    line-height: 14px;
    font-weight: 600;
    height: 18px;
    padding: 2px 5px;
    border-radius: 5px;
    background: var(--bg-input);
    color: var(--text-3);
    flex: none;
  }
  .pri.p0,
  .pri.p1 {
    color: var(--text);
  }
  .prog {
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-3);
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  /*
    The row's one chip (task 55): an icon that says which signal it is, then its label, so it
    never reads by colour alone. Needs you and failed (both Needs me items) get a red icon and
    edge; the label stays text-2 on Field Grey so it keeps 4.5:1. PR open and waits on stay
    neutral.
  */
  .sig {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 7px 0 6px;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg-input);
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-2);
    flex: none;
  }
  .sig :global(svg) {
    color: var(--text-3);
  }
  .sig.needs_you,
  .sig.failed {
    border-color: var(--needs-edge);
  }
  .sig.needs_you :global(svg),
  .sig.failed :global(svg) {
    color: var(--needs);
  }
  /* Maybe out of date: a dashed edge and a faded icon; the label keeps its contrast. */
  .sig.stale {
    border-style: dashed;
  }
  .sig.stale :global(svg) {
    opacity: 0.45;
  }
  .sig .n {
    display: none;
    font-variant-numeric: tabular-nums;
  }
  .sp {
    flex: 1;
    min-width: 8px;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 8px;
    border-radius: 11px;
    border: 1px solid var(--line-2);
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-2);
  }
  .short {
    display: none;
    align-items: center;
    gap: 4px;
  }
  .rd {
    display: none;
    align-items: center;
    gap: 4px;
    flex: none;
  }
  .when {
    font-variant-numeric: tabular-nums;
    width: 44px;
    text-align: right;
    color: var(--text-3);
    font-size: var(--fs-meta);
    flex: none;
  }
  /*
    Narrow widths, in two steps (DESIGN "Narrow widths"). The steps sit at 700px and 560px of
    list width so the Paper screens (a 728px list beside the panel) keep their repo pills; see
    PLAN-ui.md "Decisions".
  */
  @container list (max-width: 700px) {
    .rn {
      display: none;
    }
    .rd {
      display: flex;
    }
    .when {
      width: 32px;
    }
  }
  @container list (max-width: 560px) {
    .long {
      display: none;
    }
    .short {
      display: inline;
    }
    .pc {
      display: none;
    }
  }
  /* Beside the panel in a small window: the title keeps its room; the chip keeps its icon and
     count, with its label in the tooltip and the row's accessible name. */
  @container list (max-width: 460px) {
    .sig .lab {
      display: none;
    }
    .sig .n {
      display: inline;
    }
    .sig {
      padding: 0 6px;
    }
    .when {
      width: 28px;
    }
  }
  @container list (max-width: 360px) {
    .row {
      gap: 8px;
    }
    .id {
      width: 30px;
    }
    .t {
      min-width: 40px;
    }
    .rd,
    .meta {
      display: none;
    }
  }
</style>
