<script lang="ts">
  import Popover from '../../../lib/components/Popover.svelte';
  import Button from '../../../lib/components/Button.svelte';
  import Icon from '../../../lib/components/Icon.svelte';
  import type { TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import type { Anchor } from '../../../lib/state/ui.svelte';
  import { classify } from '../../../lib/logic/links';

  /**
   * ⌘L: "Paste a link or path" and an optional title; below, my open PRs from this task's
   * branch, one click each. A PR's title and state fill in when the lookup answers.
   */
  let { task, at }: { task: TaskId; at: Anchor } = $props();
  const app = useApp();
  let text = $state('');
  let title = $state('');
  let error = $state<string | null>(null);
  const kind = $derived(classify(text));
  const suggested = $derived(app.detail.detail?.task.id === task ? app.detail.detail.suggested_prs : []);

  async function add(value = text, t: string | null = title || null) {
    if (!value.trim()) return;
    error = await app.addLink(task, value, t);
    if (!error) app.close();
  }
</script>

<Popover x={Math.min(at.x, window.innerWidth - 380)} y={at.y} width={360} label="Add a link to {task}" onclose={() => app.close()}>
  <form class="f" onsubmit={(e) => (e.preventDefault(), add())}>
    <input bind:value={text} data-autofocus placeholder="Paste a link or path" autocomplete="off" spellcheck="false" aria-label="Link or path" aria-invalid={!!error} oninput={() => (error = null)} />
    <input bind:value={title} placeholder="Title (optional)" autocomplete="off" aria-label="Title" />
    {#if error}<p class="err" role="alert">{error}</p>{:else if text.trim()}<p class="kind">{kind ? kind.label : 'Not a link or a path'}</p>{/if}
    {#if suggested.length}
      <div class="sugg">
        <span class="h">Your open PRs from this branch</span>
        {#each suggested as pr (pr.ref)}
          <button type="button" class="pr" onclick={() => add(pr.url ?? pr.ref, null)}><Icon name="pr" size={13} />{pr.title ?? pr.ref}<span>{pr.ref}</span></button>
        {/each}
      </div>
    {/if}
    <div class="row">
      <Button variant="ghost" size="sm" onclick={() => app.close()}>Cancel</Button>
      <Button variant="primary" size="sm" type="submit" disabled={!text.trim()}>Add link</Button>
    </div>
  </form>
</Popover>

<style>
  .f {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 6px;
  }
  input {
    height: 30px;
    padding: 0 9px;
    border-radius: var(--r-md);
    background: var(--bg-input);
    border: 1px solid var(--line-2);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  input[aria-invalid='true'] {
    border-color: var(--st-blocked);
  }
  .err {
    font-size: var(--fs-meta);
    color: var(--warn-text);
  }
  .kind {
    font-size: var(--fs-meta);
    color: var(--text-3);
  }
  .sugg {
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-top: 1px solid var(--line);
    padding-top: 8px;
  }
  .h {
    font-size: var(--fs-label);
    color: var(--text-3);
    padding: 0 2px 4px;
  }
  .pr {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--r-sm);
    color: var(--text);
    overflow: hidden;
    white-space: nowrap;
  }
  .pr span {
    margin-left: auto;
    color: var(--text-3);
    font-size: var(--fs-label);
  }
  .pr:hover {
    background: var(--bg-sel);
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
