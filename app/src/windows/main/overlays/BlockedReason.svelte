<script lang="ts">
  import Popover from '../../../lib/components/Popover.svelte';
  import Button from '../../../lib/components/Button.svelte';
  import type { TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import type { Anchor } from '../../../lib/state/ui.svelte';

  /** Blocked asks why, in one line. ↩ saves; an empty reason can't be saved. */
  let { task, at }: { task: TaskId; at: Anchor } = $props();
  const app = useApp();
  // svelte-ignore state_referenced_locally
  let reason = $state(app.row(task)?.blocked_reason ?? '');

  function save() {
    if (!reason.trim()) return;
    app.close();
    void app.setStatus(task, 'blocked', reason.trim());
  }
</script>

<Popover x={at.x} y={at.y} width={320} label="Why is {task} blocked?" onclose={() => app.close()}>
  <form class="f" onsubmit={(e) => (e.preventDefault(), save())}>
    <label for="blocked-reason">Why is it blocked?</label>
    <input id="blocked-reason" bind:value={reason} data-autofocus placeholder="Waiting for test keys, a review, a decision…" autocomplete="off" />
    <div class="row">
      <span class="hint">{reason.trim() ? '' : 'A reason is needed.'}</span>
      <Button variant="ghost" size="sm" onclick={() => app.close()}>Cancel</Button>
      <Button variant="primary" size="sm" type="submit" disabled={!reason.trim()}>Mark blocked</Button>
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
  label {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--text-2);
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
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .hint {
    flex: 1;
    font-size: var(--fs-meta);
    color: var(--text-3);
  }
</style>
