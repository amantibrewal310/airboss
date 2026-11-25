<script lang="ts">
  import StatusIcon from '../../../lib/components/StatusIcon.svelte';
  import RepoDot from '../../../lib/components/RepoDot.svelte';
  import type { RunMap } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { layoutRunMap, moveOnMap } from '../../../lib/logic/runMapLayout';
  import { shapeOf, SHAPE_LABEL } from '../../../lib/logic/views';

  /**
   * The run-order map: waves as columns, stations in sibling order, lines from blocker to task.
   * Hover traces a station's lines; click selects (then ⌘↩ starts it); double-click opens it.
   * With the map focused: ←→ across waves, ↑↓ within a wave, ↩ opens.
   */
  let { map, width }: { map: RunMap; width: number } = $props();
  const app = useApp();
  const layout = $derived(layoutRunMap(map, width));
  const outside = $derived(new Map(map.outside.map((o) => [o.task, o.blockers])));
  let hover = $state<number | null>(null);
  const trace = $derived(hover ?? app.ui.mapSel);

  function sub(id: number): { text: string; tone?: 'ok' | 'warn' } {
    const r = app.row(id);
    if (!r) return { text: '' };
    const out = outside.get(id);
    if (out) return { text: `waits outside: ${out.join(', ')}` };
    if (r.status === 'blocked') return { text: 'blocked', tone: 'warn' };
    if (r.ready) return { text: 'ready', tone: 'ok' };
    if (r.waiting) return { text: `waits on ${r.waits_on.join(', ')}` };
    return { text: SHAPE_LABEL[shapeOf(r)].toLowerCase() };
  }

  function label(id: number, wave: number): string {
    const r = app.row(id);
    if (!r) return String(id);
    const s = sub(id).text;
    return `${id} ${r.title}, ${s}, wave ${wave + 1}`;
  }

  function key(e: KeyboardEvent) {
    const k = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' }[e.key] as 'up' | 'down' | 'left' | 'right' | undefined;
    if (k && !e.metaKey && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      app.ui.mapSel = moveOnMap(map, app.ui.mapSel, k);
      app.ui.keyboard = true;
    } else if (e.key === 'Enter' && !e.metaKey && app.ui.mapSel !== null) {
      e.preventDefault();
      e.stopPropagation();
      app.openPage(app.ui.mapSel);
    }
  }
</script>

<div class="scroll">
  <div
    class="ro"
    class:focusing={trace !== null}
    style:width="{layout.width}px"
    style:height="{layout.height}px"
    role="toolbar"
    aria-label="Run-order map. Arrow keys move between stations; Return opens one."
    tabindex="0"
    onkeydown={key}
  >
    <svg width={layout.width} height={layout.height} aria-hidden="true">
      {#each layout.dividers as x (x)}<line x1={x} y1="0" x2={x} y2={layout.height} stroke="var(--line)" />{/each}
      {#each layout.paths as p (`${p.from}>${p.to}`)}
        <path
          class:hl={trace !== null && (p.to === trace || p.from === trace)}
          d={p.d}
          fill="none"
          stroke={p.done || p.outside ? 'var(--text-3)' : 'var(--text-2)'}
          stroke-width="1.75"
          stroke-linecap="round"
          stroke-dasharray={p.done ? undefined : '2 5'}
        />
      {/each}
    </svg>
    {#each layout.heads as h (h.label)}<div class="colh" style:left="{h.x}px">{h.label}</div>{/each}
    {#each layout.stations as s (s.id)}
      {@const r = app.row(s.id)}
      {#if r}
        {@const info = sub(s.id)}
        <div class="st" style:left="{s.x - 8}px" style:top="{s.y - 8}px" aria-hidden="true"><StatusIcon shape={shapeOf(r)} label={false} /></div>
        <button
          type="button"
          class="stop"
          class:sel={app.ui.mapSel === s.id}
          class:kb={app.ui.mapSel === s.id && app.ui.keyboard}
          class:wait={r.waiting || outside.has(s.id)}
          style:left="{s.x - 8}px"
          style:top="{s.y + 11}px"
          style:width="{layout.column - 14}px"
          tabindex="-1"
          aria-label={label(s.id, s.wave)}
          aria-pressed={app.ui.mapSel === s.id}
          onpointerenter={() => (hover = s.id)}
          onpointerleave={() => (hover = null)}
          onclick={() => {
            app.ui.mapSel = s.id;
            app.ui.keyboard = false;
          }}
          ondblclick={() => app.openPage(s.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            app.ui.mapSel = s.id;
            app.open({ kind: 'context', task: s.id, at: { x: e.clientX, y: e.clientY } });
          }}
        >
          <b title={r.title}>{r.title}</b>
          <small><span>{s.id} · <span class={info.tone}>{info.text}</span></span>{#each r.repos as repo (repo)}<RepoDot id={repo} name={app.tasks.repoById.get(repo)?.name} />{/each}</small>
        </button>
      {/if}
    {/each}
  </div>
</div>
<div class="legend">
  <span><svg width="18" height="4" aria-hidden="true"><path d="M1 2H17" stroke="var(--text-3)" stroke-width="1.75" stroke-linecap="round" /></svg>done, can follow</span>
  <span><svg width="18" height="4" aria-hidden="true"><path d="M1 2H17" stroke="var(--text-3)" stroke-width="1.75" stroke-linecap="round" stroke-dasharray="2 5" /></svg>still waiting</span>
  <span>Hover a task to trace its lines · double-click to open</span>
</div>
{#if map.tangled.length}
  <p class="tangled">These wait on each other in a loop: {map.tangled.join(', ')}.</p>
{/if}

<style>
  .scroll {
    overflow-x: auto;
    scrollbar-width: thin;
  }
  .ro {
    position: relative;
    outline: none;
    border-radius: var(--r-md);
  }
  .ro:focus-visible {
    box-shadow: 0 0 0 2px var(--focus);
  }
  .ro > svg {
    position: absolute;
    inset: 0;
    overflow: visible;
  }
  path {
    transition:
      opacity 150ms var(--ease),
      stroke-width 150ms var(--ease);
  }
  .focusing path:not(.hl) {
    opacity: 0.18;
  }
  path.hl {
    stroke-width: 2.5;
  }
  .colh {
    position: absolute;
    top: 0;
    font-size: var(--fs-label);
    font-weight: 500;
    line-height: 14px;
    color: var(--text-3);
  }
  .st {
    position: absolute;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--bg);
    box-shadow: 0 0 0 3px var(--bg);
    z-index: 2;
    pointer-events: none;
  }
  .stop {
    position: absolute;
    z-index: 1;
    padding: 2px 4px;
    margin-left: -4px;
    border-radius: var(--r-sm);
    display: flex;
    flex-direction: column;
    gap: 1px;
    text-align: left;
  }
  .stop:hover {
    background: var(--bg-hover);
  }
  .stop.sel {
    background: var(--accent-soft);
  }
  .stop.kb {
    box-shadow: inset 0 0 0 var(--kb-outline) var(--focus);
  }
  .stop b {
    display: block;
    font-weight: 500;
    font-size: var(--fs-control);
    line-height: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .stop.wait b {
    color: var(--text-2);
  }
  .stop small {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
    font-size: var(--fs-label);
    line-height: 14px;
    white-space: nowrap;
    overflow: hidden;
    font-variant-numeric: tabular-nums;
  }
  .ok {
    color: var(--st-review);
  }
  .warn {
    color: var(--st-blocked);
  }
  .legend {
    display: flex;
    gap: 16px;
    padding-top: 4px;
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-3);
    flex-wrap: wrap;
  }
  .legend span {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tangled {
    font-size: var(--fs-meta);
    color: var(--st-blocked);
    margin-top: 8px;
  }
</style>
