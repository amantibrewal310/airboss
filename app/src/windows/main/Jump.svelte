<script lang="ts">
  import Modal from '../../lib/components/Modal.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import RepoDot from '../../lib/components/RepoDot.svelte';
  import type { RowDto, TaskId } from '../../lib/types';
  import { useApp } from '../../lib/state/context';
  import { COMMANDS, type Command } from '../../lib/logic/commands';
  import { invalidTarget, searchCommands, searchTasks } from '../../lib/logic/search';
  import { shapeOf } from '../../lib/logic/views';

  /**
   * ⌘K: jump to a task or run a command. ↩ on a task reveals it; on a command runs it; with no
   * match, ↩ creates the text as a task. Picker mode serves Add waits-on (⌘D) and Move under.
   */
  let { mode = 'jump', task = null }: { mode?: 'jump' | 'waits_on' | 'move_under'; task?: TaskId | null } = $props();
  const app = useApp();
  let query = $state('');
  let hl = $state(0);
  let results: HTMLElement | undefined = $state();

  type Item =
    | { kind: 'task'; row: RowDto; reason: string | null }
    | { kind: 'top' }
    | { kind: 'command'; cmd: Command }
    | { kind: 'create'; title: string };

  const palette = COMMANDS.filter((c) => c.palette);
  const items = $derived.by((): Item[] => {
    const tasks = searchTasks(app.tasks.rows, app.tasks.order, query);
    if (mode !== 'jump' && task !== null) {
      const list: Item[] = tasks
        .filter((r) => r.id !== task || query.trim())
        .map((row) => ({ kind: 'task', row, reason: invalidTarget(app.tasks.rows, mode, task, row.id) }));
      if (mode === 'move_under' && !query.trim()) list.unshift({ kind: 'top' });
      return list;
    }
    const list: Item[] = tasks.map((row) => ({ kind: 'task', row, reason: null }));
    for (const cmd of searchCommands(palette, query)) list.push({ kind: 'command', cmd });
    if (!list.length && query.trim()) list.push({ kind: 'create', title: query.trim() });
    return list;
  });
  const taskCount = $derived(items.filter((i) => i.kind === 'task' || i.kind === 'top').length);

  $effect(() => {
    void query;
    hl = 0;
  });

  $effect(() => {
    results?.querySelector(`[data-i="${hl}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  const placeholder = $derived(
    mode === 'waits_on' ? `Choose what ${task} waits on…` : mode === 'move_under' ? `Move ${task} under…` : 'Jump to a task, or type a command…',
  );

  function run(item: Item | undefined) {
    if (!item) return;
    if (item.kind === 'task' && item.reason) return;
    app.close();
    if (item.kind === 'top' && task !== null) return void app.moveUnder(task, null);
    if (item.kind === 'task') {
      if (mode === 'waits_on' && task !== null) return void app.addDependency(task, item.row.id);
      if (mode === 'move_under' && task !== null) return void app.moveUnder(task, item.row.id);
      return app.reveal(item.row.id);
    }
    if (item.kind === 'command') return app.runCommand(item.cmd.id);
    if (item.kind === 'create') void app.create({ title: item.title, body: null, parent: null, repos: [], links: [], then: 'none' });
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const n = items.length;
      if (n) hl = (hl + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(items[hl]);
    }
  }
</script>

<Modal label={mode === 'jump' ? 'Jump' : placeholder} top="13%" width={640} onclose={() => app.close()}>
  <div class="in">
    <Icon name="search" />
    <input
      bind:value={query}
      data-autofocus
      placeholder={placeholder}
      autocomplete="off"
      spellcheck="false"
      role="combobox"
      aria-expanded="true"
      aria-controls="jump-results"
      aria-activedescendant={items.length ? `jump-${hl}` : undefined}
      onkeydown={key}
    />
  </div>
  <div class="res" id="jump-results" role="listbox" aria-label="Results" bind:this={results}>
    {#each items as item, i (i)}
      {#if i === 0 && (item.kind === 'task' || item.kind === 'top')}
        <div class="grp" role="presentation">{mode !== 'jump' ? 'Tasks' : query.trim() ? 'Tasks' : 'Jump to'}</div>
      {:else if i === taskCount && item.kind === 'command'}
        <div class="grp" role="presentation">Commands</div>
      {/if}
      <div
        id="jump-{i}"
        class="it"
        class:hl={i === hl}
        class:off={item.kind === 'task' && !!item.reason}
        role="option"
        aria-selected={i === hl}
        aria-disabled={item.kind === 'task' && item.reason ? 'true' : undefined}
        title={item.kind === 'task' && item.reason ? item.reason : undefined}
        data-i={i}
        tabindex="-1"
        onpointermove={() => (hl = i)}
        onclick={() => run(item)}
        onkeydown={() => {}}
      >
        {#if item.kind === 'task'}
          <StatusIcon shape={shapeOf(item.row)} />
          <span class="id">{item.row.id}</span>
          <span class="t" title={item.row.title}>{item.row.title}</span>
          {#if item.reason}<span class="why">{item.reason}</span>{/if}
          {#each item.row.repos as r (r)}<RepoDot id={r} name={app.tasks.repoById.get(r)?.name} />{/each}
        {:else if item.kind === 'top'}
          <Icon name="list" />
          <span class="t">Top level</span>
        {:else if item.kind === 'command'}
          <span class="cmd"><Icon name="command" stroke={1.4} /></span>
          <span class="t">{item.cmd.label}</span>
          {#if item.cmd.bind[0]}<Kbd keys={item.cmd.bind[0][0]} />{/if}
        {:else}
          <Icon name="plus" />
          <span class="t">Create task “{item.title}”</span>
          <Kbd keys="enter" />
        {/if}
      </div>
    {:else}
      <div class="none">No tasks match.</div>
    {/each}
  </div>
  <div class="ft">
    <span><Kbd keys="up down" /> move</span>
    <span><Kbd keys="enter" /> {mode === 'jump' ? 'open' : 'choose'}</span>
    <span><Kbd keys="esc" /> close</span>
  </div>
</Modal>

<style>
  .in {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 52px;
    padding: 0 16px;
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
  }
  input {
    flex: 1;
    height: 100%;
    outline: none;
    font-size: var(--fs-field);
    color: var(--text);
  }
  .res {
    max-height: 380px;
    overflow-y: auto;
    padding: 6px;
    scrollbar-width: thin;
  }
  .grp {
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-3);
    padding: 8px 10px 4px;
  }
  .it {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 38px;
    padding: 0 10px;
    border-radius: var(--r-item);
    outline: none;
    line-height: 16px;
  }
  .it.hl {
    background: var(--bg-sel);
  }
  .it.off {
    color: var(--text-3);
  }
  .it.off :global(svg) {
    opacity: 0.5;
  }
  .id {
    width: 30px;
    flex: none;
    font-size: var(--fs-meta);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .why {
    font-size: var(--fs-label);
    color: var(--text-3);
    white-space: nowrap;
  }
  .cmd {
    color: var(--text-2);
    display: flex;
  }
  .none {
    padding: 24px;
    text-align: center;
    color: var(--text-3);
  }
  .ft {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 14px;
    border-top: 1px solid var(--line);
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-3);
  }
  .ft span {
    display: flex;
    align-items: center;
    gap: 5px;
  }
</style>
