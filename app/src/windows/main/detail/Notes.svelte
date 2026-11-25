<script lang="ts">
  import { tick } from 'svelte';
  import { useApp } from '../../../lib/state/context';
  import type { TaskId } from '../../../lib/types';

  /** The notes body in Notes type. Click to edit in an auto-growing textarea; ⌘↩ or blur saves, Esc cancels. */
  let { task, body, empty = 'No notes yet. Agents add notes here as they work.' }: { task: TaskId; body: string | null; empty?: string } = $props();
  const app = useApp();
  let editing = $state(false);
  let area: HTMLTextAreaElement | undefined = $state();
  let view: HTMLDivElement | undefined = $state();
  let cancelled = false;

  /** Back to the notes, not to <body>, once the edit ends. */
  async function refocus() {
    await tick();
    view?.focus({ preventScroll: true });
  }

  /**
   * A single click edits, unless it selected text (the notes can be selected and copied): a
   * drag or a double-click selects instead, so the edit waits a moment to see which it was.
   */
  let pending: ReturnType<typeof setTimeout> | null = null;
  function click(e: MouseEvent) {
    if (pending) clearTimeout(pending);
    pending = null;
    if (e.detail > 1) return;
    pending = setTimeout(() => {
      pending = null;
      const sel = window.getSelection();
      if (sel && !sel.isCollapsed && view?.contains(sel.anchorNode)) return;
      void edit();
    }, 220);
  }

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
    void refocus();
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
      void refocus();
    }
  }
</script>

{#if editing}
  <textarea bind:this={area} class="doc edit" value={body ?? ''} aria-label="Notes" oninput={grow} onkeydown={key} onblur={save}></textarea>
{:else}
  <div
    bind:this={view}
    class="doc view"
    class:empty={!body}
    role="button"
    tabindex="0"
    aria-label="Notes. Press Return to edit."
    title="Click to edit the notes"
    onclick={click}
    onkeydown={(e) => {
      if ((e.key === 'Enter' || e.key === ' ') && !e.metaKey) {
        e.preventDefault();
        e.stopPropagation();
        void edit();
      }
    }}
  >
    {body || empty}
  </div>
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
  .view {
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
    outline: none;
  }
  .view:hover {
    background: var(--bg-hover);
  }
  .view:focus-visible {
    box-shadow: 0 0 0 2px var(--focus);
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
