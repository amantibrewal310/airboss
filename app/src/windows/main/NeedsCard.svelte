<script lang="ts">
  import { tick } from 'svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import type { NeedsItem, TaskId } from '../../lib/types';
  import { useApp } from '../../lib/state/context';
  import { KIND_ICON, KIND_LABEL, mainLabel } from '../../lib/logic/needs';
  import { relative } from '../../lib/logic/time';
  import Props from './detail/Props.svelte';
  import Notes from './detail/Notes.svelte';
  import Activity from './detail/Activity.svelte';
  import Skeleton from './detail/Skeleton.svelte';

  /** The card: kind, time and task; the headline at 17px; the actions; then the task itself. */
  let { item }: { item: NeedsItem } = $props();
  const app = useApp();
  const row = $derived(app.row(item.task));
  const detail = $derived(app.detail.detail);
  const fresh = $derived(detail !== null && detail.task.id === item.task);
  /** Proposals still listed (a reject hides its row here at once). */
  const proposals = $derived(item.proposed.filter(([id]) => !app.tasks.hidden.has(id) && app.row(id)?.status === 'proposed'));

  let editing = $state<TaskId | null>(null);
  let editInput: HTMLInputElement | undefined = $state();

  async function edit(id: TaskId) {
    editing = id;
    await tick();
    editInput?.focus();
    editInput?.select();
  }

  function editKey(e: KeyboardEvent, id: TaskId) {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      const title = editInput?.value ?? '';
      editing = null;
      void app.editAndAccept(item, id, title);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      editing = null;
    }
  }

  const secondary = $derived(
    item.kind === 'proposed'
      ? { label: 'Reject all', run: () => app.rejectProposals(item, proposals.map(([id]) => id)) }
      : item.kind === 'quiet'
        ? { label: 'Snooze 3 days', run: () => app.snooze(item) }
        : null,
  );
</script>

<div class="wrap">
  <div class="big" class:stale={item.stale}>
    <div class="k">
      <span class="kic"><Icon name={KIND_ICON[item.kind]} size={14} /></span>
      <span>
        {KIND_LABEL[item.kind]} · {relative(item.since, app.now)} ago ·
        <button type="button" class="task" onclick={() => app.reveal(item.task)}>{item.task} {item.task_title}</button>
        {#if item.stale}<span class="stale-note">{' · May be out of date'}</span>{/if}
      </span>
    </div>
    <p class="q">{item.headline}</p>
    {#if item.kind === 'proposed' && proposals.length}
      <div class="props-list">
        {#each proposals as [id, title] (id)}
          <div class="prop-row">
            <StatusIcon shape="proposed" />
            {#if editing === id}
              <input bind:this={editInput} class="edit" value={app.row(id)?.title ?? title} aria-label="Edit {id}, then accept" onkeydown={(e) => editKey(e, id)} onblur={() => (editing = null)} />
            {:else}
              <span class="t" title={app.row(id)?.title ?? title}>{app.row(id)?.title ?? title}</span>
              <button type="button" class="g" onclick={() => app.acceptProposals(item, [id])}><Icon name="check" size={13} />Accept</button>
              <button type="button" class="g" onclick={() => edit(id)}>Edit</button>
              <button type="button" class="g x" aria-label="Reject {id}" title="Reject" onclick={() => app.rejectProposals(item, [id])}><Icon name="close" size={13} /></button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    <div class="acts">
      <button type="button" class="pri" onclick={() => app.mainAction(item)}>{mainLabel(item.kind)} <Kbd keys="enter" onFill /></button>
      {#if secondary}<button type="button" class="sec" onclick={secondary.run}>{secondary.label}</button>{/if}
      <span class="sp"></span>
      <button type="button" class="ghost" onclick={() => app.dismiss(item, item.kind === 'finish_it' ? 'Kept open' : 'Dismissed')}>
        {item.kind === 'finish_it' ? 'Keep open' : 'Dismiss'} <Kbd keys="mod+backspace" joined />
      </button>
    </div>
  </div>

  {#if row}
    <div class="task-sec">
      {#if app.detail.skeleton && !fresh}
        <Skeleton />
      {:else}
        <Props {row} detail={fresh ? detail : null} />
        <Notes task={row.id} body={fresh ? detail!.task.body : null} empty="No notes on this task." />
        {#if fresh}<Activity task={row.id} history={detail!.history} more={detail!.history_more} />{/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .wrap {
    padding: 24px 28px 40px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .big {
    background: var(--bg-raised);
    border: 1px solid var(--line-2);
    border-radius: var(--r-xl);
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
  }
  .big.stale .q {
    color: var(--text-2);
  }
  .k {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .kic {
    color: var(--needs);
    display: flex;
  }
  .task {
    color: var(--text);
  }
  .task:hover {
    text-decoration: underline;
  }
  .stale-note {
    color: var(--text-3);
  }
  .q {
    font-size: var(--fs-title);
    font-weight: 500;
    letter-spacing: var(--track-title);
    line-height: 24px;
    padding: 10px 0 4px;
    overflow-wrap: anywhere;
  }
  .props-list {
    margin-top: 12px;
  }
  .prop-row {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 36px;
    border-top: 1px solid var(--line);
  }
  .prop-row:first-child {
    border-top: 0;
  }
  .t {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .edit {
    flex: 1;
    height: 26px;
    padding: 0 7px;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    outline: none;
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 16px;
  }
  .sp {
    flex: 1;
  }
  button.pri,
  button.sec,
  button.ghost,
  button.g {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 0 11px;
    border-radius: var(--r-md);
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
    border: 1px solid transparent;
    white-space: nowrap;
  }
  .pri {
    background: var(--accent-fill);
    color: var(--on-fill);
    padding-right: 10px;
  }
  .pri:hover {
    filter: brightness(1.1);
  }
  .sec {
    background: var(--bg-input);
    border-color: var(--line-2) !important;
    color: var(--text);
  }
  .sec:hover {
    background: var(--bg-sel);
  }
  .ghost,
  .g {
    color: var(--text-2);
    padding: 0 10px;
  }
  .ghost:hover,
  .g:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .g.x {
    padding: 0 8px;
  }
  .task-sec {
    max-width: 720px;
    padding-top: 4px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
</style>
