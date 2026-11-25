<script lang="ts">
  import { createRawSnippet } from 'svelte';
  import Menu, { type MenuEntry } from '../../../lib/components/Menu.svelte';
  import type { TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import type { Anchor } from '../../../lib/state/ui.svelte';
  import { dotHtml } from './shapes';

  /** R or "Link repo": a checklist of repos; ↩ or click toggles. The last line is "Add repos…". */
  let { task, at }: { task: TaskId; at: Anchor } = $props();
  const app = useApp();
  const row = $derived(app.row(task));
  const own = $derived(row && !row.repos_from_subtasks ? row.repos : []);

  const items: MenuEntry[] = $derived([
    ...app.tasks.repos.map(
      (r): MenuEntry => ({
        id: `r${r.id}`,
        label: r.name,
        checked: own.includes(r.id),
        stay: true,
        icon: createRawSnippet(() => ({ render: () => `<span style="display:flex">${dotHtml(r.id)}</span>` })),
        run: () => void app.toggleRepo(task, r.id, !own.includes(r.id)),
      }),
    ),
    'separator',
    { id: 'add', label: 'Add repos…', run: () => app.open({ kind: 'add_repos' }) },
  ]);
</script>

<Menu {items} x={at.x} y={at.y} width={220} label="Link repos to {task}" checklist onclose={() => app.close()} />
