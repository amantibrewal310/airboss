<script lang="ts">
  import { tick } from 'svelte';
  import { useApp } from '../../../lib/state/context';
  import type { TaskId } from '../../../lib/types';

  /** The task title: click it (or press ↩ on it) to rename in place. ↩ or blur saves; Esc cancels. */
  let { task, title, size = 'panel' }: { task: TaskId; title: string; size?: 'panel' | 'page' } = $props();
  const app = useApp();
  let editing = $state(false);
  let input: HTMLTextAreaElement | undefined = $state();
  let cancelled = false;

  async function edit() {
    editing = true;
    cancelled = false;
    await tick();
    if (input) {
      grow();
      input.focus();
      input.select();
    }
  }

  function grow() {
    if (!input) return;
    input.style.height = 'auto';
    input.style.height = `${input.scrollHeight}px`;
  }

  function save() {
    if (!editing) return;
    editing = false;
    if (!cancelled && input) void app.rename(task, input.value.replace(/\n/g, ' '));
  }

  function key(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      input?.blur();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelled = true;
      editing = false;
    }
  }
</script>

{#if editing}
  <textarea bind:this={input} class="t {size} edit" value={title} rows="1" aria-label="Rename {task}" oninput={grow} onkeydown={key} onblur={save}></textarea>
{:else}
  <h1 class="t {size}"><button type="button" title="Rename" onclick={edit}>{title}</button></h1>
{/if}

<style>
  .t {
    font-size: var(--fs-headline);
    font-weight: 600;
    letter-spacing: var(--track-display);
    line-height: 26px;
    overflow-wrap: anywhere;
  }
  .page {
    font-size: var(--fs-display);
    line-height: 32px;
  }
  h1 button {
    text-align: left;
    border-radius: var(--r-sm);
    padding: 0 4px;
    margin: 0 -4px;
  }
  h1 button:hover {
    background: var(--bg-hover);
  }
  .edit {
    width: calc(100% + 16px);
    margin: -4px -8px;
    padding: 3px 7px;
    resize: none;
    overflow: hidden;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: var(--r-md);
    outline: none;
  }
</style>
