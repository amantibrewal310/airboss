<script lang="ts">
  import Modal from '../../../lib/components/Modal.svelte';
  import Button from '../../../lib/components/Button.svelte';
  import type { TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';

  /** Dropped confirms first: "Drop 142 and its 3 open subtasks?". Drop is the default; Esc cancels. */
  let { task, open }: { task: TaskId; open: number } = $props();
  const app = useApp();
  const title = $derived(app.row(task)?.title ?? '');
</script>

<Modal label="Drop {task}" top="24%" width={400} onclose={() => app.close()}>
  <div class="c">
    <h2>Drop {task}{open ? ` and its ${open} open subtask${open === 1 ? '' : 's'}` : ''}?</h2>
    <p>“{title}” leaves your lists. ⌘Z brings it back.</p>
    <div class="row">
      <Button variant="ghost" onclick={() => app.close()}>Cancel</Button>
      <Button
        variant="primary"
        data-autofocus
        onclick={() => {
          app.close();
          void app.drop(task);
        }}>Drop</Button
      >
    </div>
  </div>
</Modal>

<style>
  .c {
    padding: 20px 20px 16px;
  }
  h2 {
    font-size: var(--fs-field);
    font-weight: 600;
    line-height: 20px;
  }
  p {
    margin-top: 6px;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }
</style>
