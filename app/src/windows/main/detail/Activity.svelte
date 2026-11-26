<script lang="ts">
  import Avatar from '../../../lib/components/Avatar.svelte';
  import type { HistoryEntry, LinkDto, TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { phrase, type PhraseContext } from '../../../lib/logic/activity';
  import { linkLabel } from '../../../lib/logic/links';
  import { feedTime } from '../../../lib/logic/time';

  /** The activity feed (with subtasks' events) and the note input. ↩ adds the note, ⇧↩ adds a line. */
  let { task, history, more, links = [] }: { task: TaskId; history: HistoryEntry[]; more: boolean; links?: LinkDto[] } = $props();
  const app = useApp();
  /** Thread ref or title → the folder it runs in, for "Started thread slot-picker in booking-web". */
  const folders = $derived.by(() => {
    const m = new Map<string, string>();
    for (const l of links) {
      const path = l.thread?.project_path;
      if (!path) continue;
      const folder = path.replace(/\/+$/, '').split('/').pop();
      if (!folder) continue;
      m.set(l.ref, folder);
      if (l.thread?.title) m.set(l.thread.title, folder);
    }
    return m;
  });
  const ctx: PhraseContext = {
    repoOf: (thread) => folders.get(thread) ?? null,
    linkTitle: (kind, ref) => {
      const l = links.find((x) => x.kind === kind && x.ref === ref);
      return l ? linkLabel(l) : null;
    },
    threadTitle: (ref) => {
      const l = links.find((x) => x.kind === 't3_thread' && x.ref === ref);
      return l?.thread?.title ?? l?.title ?? null;
    },
  };
  let note = $state('');
  let area: HTMLTextAreaElement | undefined = $state();

  function grow() {
    if (!area) return;
    area.style.height = 'auto';
    area.style.height = `${Math.max(38, area.scrollHeight + 2)}px`;
  }

  async function key(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      if (await app.addNote(task, note)) {
        note = '';
        queueMicrotask(grow);
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      area?.blur();
    }
  }
</script>

<section class="sec" aria-label="Activity">
  <h3>Activity</h3>
  <ol class="feed">
    {#each history as e (e.id)}
      <li class="ev">
        <Avatar actor={e.actor} size={18} />
        <span class="tx">
          {#each phrase(e, task, ctx) as s, i (i)}{#if s.strong}<b>{s.text}</b>{:else}{s.text}{/if}{/each}
        </span>
        <span class="tm">{feedTime(e.at, app.now)}</span>
      </li>
    {/each}
  </ol>
  {#if more}
    <button type="button" class="more" onclick={() => app.showMoreHistory()}>Show more</button>
  {/if}
  <textarea
    bind:this={area}
    bind:value={note}
    class="note"
    rows="1"
    placeholder="Leave a note for the agent…"
    aria-label="Leave a note for the agent"
    oninput={grow}
    onkeydown={key}
  ></textarea>
</section>

<style>
  .sec {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-top: 6px;
  }
  h3 {
    font-size: var(--fs-meta);
    font-weight: 600;
    color: var(--text-2);
    line-height: 16px;
    padding-bottom: 6px;
  }
  .feed {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ev {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 30px;
    font-size: var(--fs-control);
    line-height: 16px;
    color: var(--text-2);
  }
  .tx {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    padding: 7px 0;
    /* Activity can be selected and copied. */
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }
  b {
    color: var(--text);
    font-weight: 500;
  }
  .tm {
    color: var(--text-3);
    white-space: nowrap;
    padding-left: 10px;
    font-variant-numeric: tabular-nums;
  }
  .more {
    align-self: flex-start;
    color: var(--text-3);
    font-size: var(--fs-meta);
    padding: 4px 0;
  }
  .more:hover {
    color: var(--text-2);
  }
  .note {
    margin-top: 8px;
    height: 38px;
    resize: none;
    border: 1px solid var(--line-2);
    border-radius: var(--r-lg);
    padding: 10px 11px;
    background: var(--bg-raised);
    font-size: var(--fs-control);
    line-height: 16px;
    color: var(--text);
    outline: none;
    overflow: hidden;
  }
  .note:focus {
    border-color: var(--accent);
  }
</style>
