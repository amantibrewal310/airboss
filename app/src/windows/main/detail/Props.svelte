<script lang="ts">
  import { tick } from 'svelte';
  import StatusIcon from '../../../lib/components/StatusIcon.svelte';
  import StackedBar from '../../../lib/components/StackedBar.svelte';
  import RepoDot from '../../../lib/components/RepoDot.svelte';
  import Icon from '../../../lib/components/Icon.svelte';
  import type { DepDto, RowDto, TaskDetail } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { linkLabel } from '../../../lib/logic/links';
  import { percent, shapeOf, SHAPE_LABEL } from '../../../lib/logic/views';
  import { dueLabel, isDue } from '../../../lib/logic/time';

  /**
   * The properties grid: Status, (Progress on the page), Due, Repos, Branch, Parent, Waits on,
   * Blocks, Links. 104px labels in the panel, 86px in the page's rail; 30px per line.
   */
  let { row, detail, rail = false }: { row: RowDto; detail: TaskDetail | null; rail?: boolean } = $props();
  const app = useApp();

  const shape = $derived(shapeOf(row));
  const repos = $derived(row.repos.map((id) => app.tasks.repoById.get(id)).filter((r) => !!r));
  const parent = $derived(row.parent === null ? null : app.row(row.parent));
  const prepared = $derived(detail?.task.id === row.id ? detail.prepared : null);
  const mine = $derived(detail?.task.id === row.id ? detail : null);
  /** The task's own branch. The one Do it would use otherwise is on Do it's tooltip (PLAN-ui.md, Decisions). */
  const own = $derived(mine ? mine.task.branch : null);

  let editingBranch = $state(false);
  let branchError = $state<string | null>(null);
  let branchInput: HTMLInputElement | undefined = $state();
  let branchTag: HTMLButtonElement | undefined = $state();

  async function branchDone() {
    editingBranch = false;
    await tick();
    (branchTag ?? document.querySelector<HTMLElement>('[data-zone="panel"]'))?.focus({ preventScroll: true });
  }

  async function editBranch() {
    editingBranch = true;
    branchError = null;
    await tick();
    branchInput?.focus();
    branchInput?.select();
  }

  async function saveBranch() {
    if (!editingBranch || !branchInput) return;
    const value = branchInput.value.trim();
    if (value === (own ?? '')) return void branchDone();
    const err = await app.setBranch(row.id, value);
    branchError = err;
    if (!err) void branchDone();
  }

  function branchKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      void saveBranch();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      branchError = null;
      void branchDone();
    }
  }

  let editingDue = $state(false);
  let dueError = $state<string | null>(null);
  let dueInput: HTMLInputElement | undefined = $state();
  let dueTag: HTMLButtonElement | undefined = $state();
  /** Its due date has come: shown in the Needs colour, as Needs me lists it. */
  const late = $derived(row.due_at !== null && isDue(row.due_at, app.now) && row.status !== 'done' && row.status !== 'dropped');

  async function dueDone() {
    editingDue = false;
    await tick();
    (dueTag ?? document.querySelector<HTMLElement>('[data-zone="panel"]'))?.focus({ preventScroll: true });
  }

  async function editDue() {
    editingDue = true;
    dueError = null;
    await tick();
    dueInput?.focus();
  }

  /** Saves the picked day, or clears it when the field is emptied. */
  async function saveDue() {
    if (!editingDue || !dueInput) return;
    const value = dueInput.value.trim();
    if (value === (row.due_at ?? '')) return void dueDone();
    const err = await app.setDue(row.id, value);
    dueError = err;
    if (!err) void dueDone();
  }

  function dueKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      void saveDue();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      dueError = null;
      void dueDone();
    }
  }

  function anchorOf(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4 };
  }

  const depShape = (d: DepDto) => shapeOf(d);
</script>

<div class="props" class:rail>
  <div class="p">
    <span class="l">Status</span>
    <button type="button" class="v hov" data-status-for={row.id} onclick={(e) => app.open({ kind: 'status', task: row.id, at: anchorOf(e) })}>
      <StatusIcon {shape} label={false} />
      <span>{SHAPE_LABEL[shape]}</span>
      {#if row.status === 'blocked' && row.blocked_reason}<span class="faint trunc">· {row.blocked_reason}</span>{/if}
    </button>
  </div>

  {#if rail && row.progress}
    <div class="p">
      <span class="l">Progress</span>
      <span class="v grow">
        <StackedBar p={row.progress} />
        <span class="nowrap">{percent(row.progress)}% <span class="faint">· {row.progress.done} of {row.progress.total}</span></span>
      </span>
    </div>
  {/if}

  <div class="p" class:wrap={!!dueError}>
    <span class="l">Due</span>
    <span class="v col">
      {#if editingDue}
        <input
          bind:this={dueInput}
          type="date"
          class="branch-in date-in"
          value={row.due_at ?? ''}
          aria-label="Due date"
          aria-invalid={!!dueError}
          onkeydown={dueKey}
          onblur={saveDue}
        />
        {#if dueError}<span class="err" role="alert">{dueError}</span>{/if}
      {:else if row.due_at}
        <span class="tag dep" class:late title={row.due_at}>
          <button bind:this={dueTag} type="button" class="in" title="Change the due date" onclick={editDue}><Icon name="calendar" size={12} />{dueLabel(row.due_at, app.now)}</button>
          <button type="button" class="x" aria-label="Clear the due date" onclick={() => app.setDue(row.id, '')}><Icon name="close" size={11} stroke={1.8} /></button>
        </span>
      {:else}
        <button bind:this={dueTag} type="button" class="faint link" onclick={editDue}>None · set a date</button>
      {/if}
    </span>
  </div>

  <div class="p" class:wrap={rail}>
    <span class="l">Repos</span>
    <span class="v tags">
      {#each repos as r (r!.id)}
        <button type="button" class="tag" title="Link repos (R)" onclick={(e) => app.open({ kind: 'repos', task: row.id, at: anchorOf(e) })}><RepoDot id={r!.id} />{r!.name}</button>
      {/each}
      {#if row.repos_from_subtasks}<span class="faint">· from subtasks</span>{/if}
      <!-- Inherited repos are changed on the subtasks, so a parent offers no "+ Link repo". -->
      {#if !row.repos_from_subtasks && (!rail || !repos.length)}
        <button type="button" class="add" onclick={(e) => app.open({ kind: 'repos', task: row.id, at: anchorOf(e) })}>+ {repos.length ? 'Link repo' : 'Link a repo'}</button>
      {/if}
    </span>
  </div>

  {#if own || editingBranch}
    <div class="p" class:wrap={!!branchError}>
      <span class="l">Branch</span>
      <span class="v col">
        {#if editingBranch}
          <input
            bind:this={branchInput}
            class="branch-in"
            value={own ?? prepared?.branch ?? ''}
            aria-label="Branch"
            aria-invalid={!!branchError}
            spellcheck="false"
            onkeydown={branchKey}
            onblur={saveBranch}
          />
          {#if branchError}<span class="err" role="alert">{branchError}</span>{/if}
        {:else}
          <button bind:this={branchTag} type="button" class="tag mono" title="Edit the branch" onclick={editBranch}>
            <Icon name="branch" size={12} />{own}
          </button>
        {/if}
      </span>
    </div>
  {/if}

  {#if parent}
    <div class="p">
      <span class="l">Parent</span>
      <span class="v">
        <button type="button" class="tag trunc" title="Move under another task" onclick={() => app.open({ kind: 'jump', mode: 'move_under', task: row.id })}>{parent.id} {parent.title}</button>
      </span>
    </div>
  {/if}

  <div class="p" class:wrap={rail}>
    <span class="l">Waits on</span>
    <span class="v tags">
      {#if mine?.waits_on.length}
        {#each mine.waits_on as d (d.id)}
          <span class="tag dep" title="{d.id} {d.title}">
            <button type="button" class="in" onclick={() => app.reveal(d.id)}><StatusIcon shape={depShape(d)} size={14} />{d.id}</button>
            <button type="button" class="x" aria-label="Stop waiting on {d.id}" onclick={() => app.removeDependency(row.id, d.id)}><Icon name="close" size={11} stroke={1.8} /></button>
          </span>
        {/each}
      {:else}
        <button type="button" class="faint link" onclick={() => app.open({ kind: 'jump', mode: 'waits_on', task: row.id })}>Nothing · ⌘D to add</button>
      {/if}
    </span>
  </div>

  {#if mine?.blocks.length}
    <div class="p" class:wrap={rail}>
      <span class="l">Blocks</span>
      <span class="v tags">
        {#each mine.blocks as d (d.id)}
          <button type="button" class="tag" title="{d.id} {d.title}" onclick={() => app.reveal(d.id)}><StatusIcon shape={depShape(d)} size={14} />{d.id}</button>
        {/each}
      </span>
    </div>
  {/if}

  <div class="p" class:wrap={rail}>
    <span class="l">Links</span>
    <span class="v tags">
      {#if mine?.links.length}
        {#each mine.links as l (l.id)}
          <span class="tag dep" title={l.url ?? l.ref}>
            <button type="button" class="in" onclick={() => app.openLink(l)}>{linkLabel(l)}</button>
            <button type="button" class="x" aria-label="Unlink {linkLabel(l)}" onclick={() => app.removeLink(l.id)}><Icon name="close" size={11} stroke={1.8} /></button>
          </span>
        {/each}
      {:else}
        <button type="button" class="faint link" onclick={() => app.runCommand('add_link')}>None · ⌘L to add</button>
      {/if}
    </span>
  </div>
</div>

<style>
  .props {
    display: flex;
    flex-direction: column;
    font-size: var(--fs-control);
    line-height: 16px;
  }
  .rail {
    gap: 2px;
    padding-top: 4px;
  }
  .p {
    display: flex;
    align-items: center;
    min-height: 30px;
    flex: none;
  }
  .p.wrap {
    align-items: flex-start;
    padding: 4px 0;
  }
  .p.wrap .l {
    line-height: 22px;
  }
  .l {
    width: 104px;
    flex: none;
    color: var(--text-3);
  }
  .rail .l {
    width: 86px;
  }
  .v {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex: 0 1 auto;
  }
  .v.grow {
    flex: 1;
    gap: 10px;
  }
  .v.col {
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }
  .tags {
    flex-wrap: wrap;
  }
  .hov {
    border-radius: var(--r-sm);
    padding: 3px 6px;
    margin: 0 -6px;
  }
  .hov:hover {
    background: var(--bg-hover);
  }
  .trunc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .faint {
    color: var(--text-3);
  }
  .link:hover {
    color: var(--text-2);
  }
  .nowrap {
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 7px;
    border-radius: var(--r-sm);
    background: var(--bg-input);
    border: 1px solid var(--line);
    font-size: var(--fs-meta);
    color: var(--text);
    white-space: nowrap;
    max-width: 100%;
  }
  button.tag:hover,
  .dep:hover {
    border-color: var(--line-2);
  }
  .tag.mono {
    font-family: var(--mono);
    font-size: var(--fs-label);
    color: var(--text);
  }
  .tag.mono :global(svg) {
    color: var(--text-2);
  }
  .dep {
    padding: 0;
    gap: 0;
  }
  .dep .in {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 7px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dep .x {
    display: none;
    height: 100%;
    padding: 0 6px 0 0;
    margin-left: -3px;
    color: var(--text-3);
    align-items: center;
  }
  .dep:hover .x,
  .dep:focus-within .x {
    display: inline-flex;
  }
  .dep .x:hover {
    color: var(--text);
  }
  .add {
    color: var(--text-3);
    font-size: var(--fs-meta);
    padding-left: 4px;
    white-space: nowrap;
  }
  .add:hover {
    color: var(--text-2);
  }
  .branch-in {
    font-family: var(--mono);
    font-size: var(--fs-label);
    height: 24px;
    width: 260px;
    max-width: 100%;
    padding: 0 7px;
    border-radius: var(--r-sm);
    background: var(--bg-input);
    border: 1px solid var(--accent);
    outline: none;
  }
  .date-in {
    font-family: inherit;
    font-size: var(--fs-meta);
    width: 160px;
  }
  /* A due date that has come reads in the Needs colour, as its Needs me item does. */
  .dep.late {
    color: var(--needs-text);
    border-color: var(--needs-edge);
    background: var(--needs-soft);
  }
  .dep.late .in :global(svg) {
    color: var(--needs);
  }
  .branch-in[aria-invalid='true'] {
    border-color: var(--st-blocked);
  }
  .err {
    font-size: var(--fs-meta);
    color: var(--warn-text);
  }
</style>
