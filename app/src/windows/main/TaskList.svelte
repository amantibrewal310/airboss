<script lang="ts">
  import VirtualList from '../../lib/components/VirtualList.svelte';
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import TaskRow from './TaskRow.svelte';
  import { useApp } from '../../lib/state/context';
  import { isTreeLayout, parseView, type VisibleItem } from '../../lib/logic/views';

  /** The list: one tab stop with roving selection (aria-activedescendant); clicks by delegation. */
  const app = useApp();

  const tree = $derived(isTreeLayout(app.ui.view, app.ui.layout));
  const ready = $derived(app.ui.view === 'ready');
  const active = $derived(app.ui.selected === null ? -1 : app.visible.findIndex((i) => i.kind === 'row' && i.id === app.ui.selected));
  const groupOf = $derived.by(() => {
    const items = app.visible;
    const out = new Int32Array(items.length).fill(-1);
    let g = -1;
    items.forEach((it, i) => {
      if (it.kind === 'group') g = i;
      out[i] = g;
    });
    return out;
  });
  const hasGroups = $derived(app.visible.some((i) => i.kind === 'group'));

  const heightOf = (i: VisibleItem) => (i.kind === 'group' ? 34 : 38);

  function rowId(e: Event): number | null {
    const el = (e.target as HTMLElement).closest<HTMLElement>('[data-row]');
    return el ? Number(el.dataset.row) : null;
  }

  function click(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('input')) return;
    const fold = (e.target as HTMLElement).closest<HTMLElement>('[data-fold]');
    if (fold) {
      app.toggleFold(Number(fold.dataset.fold));
      return;
    }
    const id = rowId(e);
    if (id === null) return;
    const status = (e.target as HTMLElement).closest<HTMLElement>('[data-status-for]');
    app.select(id, { open: !status, keyboard: false });
    if (status) {
      const r = status.getBoundingClientRect();
      const row = app.row(id);
      if (row) app.open({ kind: 'status', task: id, at: { x: r.left - 6, y: r.bottom + 6 } });
    }
  }

  function dblclick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('[data-fold], [data-status-for], input')) return;
    const id = rowId(e);
    if (id !== null) app.openPage(id);
  }

  function contextmenu(e: MouseEvent) {
    const id = rowId(e);
    if (id === null) return;
    e.preventDefault();
    app.select(id, { keyboard: false });
    app.open({ kind: 'context', task: id, at: { x: e.clientX, y: e.clientY } });
  }

  const emptyText = $derived(
    ready ? 'No tasks match this view. Tasks show here when nothing they wait on is open.' : 'No tasks match this view.',
  );
  const kind = $derived(parseView(app.ui.view).kind);
</script>

<div class="list" class:empty={!app.visible.length}>
  {#if app.visible.length}
    <VirtualList
      items={app.visible}
      {heightOf}
      {active}
      stickyOf={hasGroups ? (i: number) => groupOf[i] ?? -1 : undefined}
      bind:scroller={app.focusTargets.list}
      tabindex="0"
      role={tree ? 'tree' : 'listbox'}
      aria-label={app.title}
      aria-activedescendant={app.ui.selected !== null && active >= 0 ? `row-${app.ui.selected}` : undefined}
      onclick={click}
      ondblclick={dblclick}
      oncontextmenu={contextmenu}
    >
      {#snippet item(it: VisibleItem)}
        {#if it.kind === 'group'}
          <div class="grp" role="presentation"><StatusIcon shape={it.shape} /><b>{it.label}</b><span>{it.count}</span></div>
        {:else}
          {@const row = app.tasks.rows.get(it.id)}
          {#if row}
            <TaskRow
              {row}
              depth={it.depth}
              repos={app.tasks.repoById}
              now={app.now}
              selected={app.ui.selected === row.id}
              keyboard={app.ui.keyboard && (app.ui.zone === 'list' || app.ui.zone === 'overlay')}
              folded={app.ui.folded.has(row.id)}
              foldable={tree}
              fresh={app.tasks.fresh.has(row.id)}
              pop={app.tasks.popped.get(row.id) ?? 0}
              renaming={app.ui.renaming === row.id}
              since={ready}
              role={tree ? 'treeitem' : 'option'}
              pos={it.pos}
              size={it.size}
              onrename={(title) => app.endRowRename(row.id, title)}
            />
          {/if}
        {/if}
      {/snippet}
      {#snippet sticky(it: VisibleItem)}
        {#if it.kind === 'group'}
          <div class="grp"><StatusIcon shape={it.shape} /><b>{it.label}</b><span>{it.count}</span></div>
        {/if}
      {/snippet}
    </VirtualList>
  {:else}
    <div class="nothing" bind:this={app.focusTargets.list} tabindex="-1">
      <Icon name="check" size={22} />
      <h2>Nothing here</h2>
      <p>{emptyText}</p>
      {#if kind === 'parent' || kind === 'repo'}<p class="hint">Press C to add a task here.</p>{/if}
    </div>
  {/if}
</div>

<style>
  .list {
    flex: 1;
    min-width: 0;
    min-height: 0;
    container-type: inline-size;
    container-name: list;
    position: relative;
  }
  .grp {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 18px;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
    background: var(--bg-raised);
    border-bottom: 1px solid var(--line);
    border-top: 1px solid var(--line);
  }
  .grp b {
    color: var(--text);
    font-weight: 500;
  }
  .nothing {
    color: var(--text-2);
    text-align: center;
    padding: 90px 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    outline: none;
  }
  .nothing h2 {
    font-size: var(--fs-title);
    font-weight: 600;
    color: var(--text);
    margin: 10px 0 4px;
  }
  .hint {
    margin-top: 8px;
    color: var(--text-3);
    font-size: var(--fs-meta);
  }
</style>
