<script lang="ts">
  import { createRawSnippet } from 'svelte';
  import Menu, { type MenuEntry } from '../../../lib/components/Menu.svelte';
  import type { Priority, TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import type { Anchor } from '../../../lib/state/ui.svelte';

  /**
   * P, or click Priority in Props: P0 (most urgent) to P3, or None. 0–3 pick a priority at once,
   * ⌫ clears it; ↑↓ and ↩ pick. It's the task's own, so a parent takes one too (SPEC-core rule 35).
   */
  let { task, at }: { task: TaskId; at: Anchor } = $props();
  const app = useApp();
  const row = $derived(app.row(task));
  const ORDER: (Priority | null)[] = ['p0', 'p1', 'p2', 'p3', null];
  const LABEL: Record<Priority, string> = { p0: 'P0 · most urgent', p1: 'P1', p2: 'P2', p3: 'P3 · least urgent' };

  /** The priority the task has now is highlighted when the menu opens. */
  const initial = $derived(Math.max(0, ORDER.indexOf(row?.priority ?? null)));

  const tag = (p: Priority | null) =>
    createRawSnippet(() => ({
      render: () =>
        `<span style="display:inline-flex;align-items:center;justify-content:center;width:22px;height:18px;border-radius:5px;background:var(--bg-input);font-size:var(--fs-label);font-weight:600;font-variant-numeric:tabular-nums;color:${p === 'p0' || p === 'p1' ? 'var(--text)' : 'var(--text-3)'}">${p ? p.toUpperCase() : '–'}</span>`,
    }));

  const items: MenuEntry[] = $derived(
    ORDER.map((p) => ({
      id: p ?? 'none',
      label: p ? LABEL[p] : 'None',
      hint: p ? p.slice(1) : '⌫',
      key: p ? p.slice(1) : 'Backspace',
      icon: tag(p),
      run: () => void app.setPriority(task, p),
    })),
  );
</script>

<Menu {items} {initial} x={at.x} y={at.y} width={200} label="Priority of #{task}" onclose={() => app.close()} />
