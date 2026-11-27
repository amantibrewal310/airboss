<script lang="ts">
  import { createRawSnippet } from 'svelte';
  import Menu, { type MenuEntry } from '../../../lib/components/Menu.svelte';
  import type { Status, TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import type { Anchor } from '../../../lib/state/ui.svelte';
  import { SHAPE_LABEL } from '../../../lib/logic/views';
  import { shapeSvg } from './shapes';

  /**
   * S, or click a status icon: 1 Todo … 6 Dropped. Number keys, ↑↓ and ↩ pick. Blocked asks why;
   * Dropped confirms. A proposed task offers Accept and Reject instead. A task with subtasks
   * shows what they add up to, so Dropped is the only one it takes (SPEC-core rule 10).
   */
  let { task, at }: { task: TaskId; at: Anchor } = $props();
  const app = useApp();
  const row = $derived(app.row(task));
  const ORDER: Status[] = ['todo', 'doing', 'in_review', 'blocked', 'done', 'dropped'];

  /** The status the task has now is highlighted when the menu opens. */
  const initial = $derived(row && row.status !== 'proposed' ? Math.max(0, ORDER.indexOf(row.status)) : 0);

  const icon = (s: Status) => createRawSnippet(() => ({ render: () => shapeSvg(s) }));

  const items: MenuEntry[] = $derived(
    row?.status === 'proposed'
      ? [
          { id: 'accept', label: 'Accept', hint: '1', icon: icon('todo'), run: () => void app.acceptOne(task) },
          { id: 'reject', label: 'Reject', hint: '2', icon: icon('dropped'), run: () => void app.rejectOne(task) },
        ]
      : ORDER.map((s, i) => ({
          id: s,
          label: SHAPE_LABEL[s],
          hint: String(i + 1),
          icon: icon(s),
          disabled: row?.has_steps && s !== 'dropped' ? 'Follows its subtasks' : false,
          run: () => void app.setStatus(task, s),
        })),
  );
</script>

<Menu {items} {initial} x={at.x} y={at.y} width={220} label="Status of #{task}" numbered onclose={() => app.close()} />
