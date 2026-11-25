<script lang="ts">
  import { tick } from 'svelte';
  import { useApp } from '../../../lib/state/context';
  import type { TaskId } from '../../../lib/types';

  /** The notes body in Notes type. Click to edit in an auto-growing textarea; ⌘↩ or blur saves, Esc cancels. */
  let { task, body, empty = 'No notes yet. Agents add notes here as they work.' }: { task: TaskId; body: string | null; empty?: string } = $props();
  const app = useApp();
  let editing = $state(false);
  let area: HTMLTextAreaElement | undefined = $state();
  let cancelled = false;

  async function edit() {
    editing = true;
    cancelled = false;
    await tick();
    if (area) {
      grow();
      area.focus();
      area.setSelectionRange(area.value.length, area.value.length);
    }
  }

  function grow() {
    if (!area) return;
    area.style.height = 'auto';
    area.style.height = `${area.scrollHeight}px`;
  }

  async function save() {
    if (!editing || cancelled) return;
    editing = false;
    const next = area?.value ?? '';
    if (next.trim() !== (body ?? '').trim()) await app.editNotes(task, next);
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'Enter' && e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      area?.blur();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancelled = true;
      editing = false;
    }
  }
</script>

{#if editing}
  <textarea bind:this={area} class="doc edit" value={body ?? ''} aria-label="Notes" oninput={grow} onkeydown={key} onblur={save}></textarea>
{:else}
  <button type="button" class="doc" class:empty={!body} title="Click to edit the notes" onclick={edit}>{body || empty}</button>
{/if}

<style>
  .doc {
    display: block;
    width: 100%;
    font-size: var(--fs-notes);
    line-height: 22px;
    color: var(--text);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    text-align: left;
    border-radius: var(--r-sm);
    padding: 2px 6px;
    margin: -2px -6px;
    width: calc(100% + 12px);
  }
  button.doc:hover {
    background: var(--bg-hover);
  }
  .empty {
    color: var(--text-3);
  }
  .edit {
    resize: none;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    outline: none;
    min-height: 66px;
    padding: 5px 8px;
    margin: -6px -9px;
    width: calc(100% + 18px);
    overflow: hidden;
  }
</style>
