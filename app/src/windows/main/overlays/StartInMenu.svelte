<script lang="ts">
  import { createRawSnippet } from 'svelte';
  import Menu, { type MenuEntry } from '../../../lib/components/Menu.svelte';
  import type { DoItPlace, Repo, TaskId } from '../../../lib/types';
  import { ICONS } from '../../../lib/components/icons';
  import { dotHtml, iconSvg } from './shapes';

  /**
   * Do it on a task with no repo: "Start in <repo>" for each repo (this links the repo, then
   * starts) and "No repo: research only". ↑↓ and ↩ pick; Esc closes.
   */
  let {
    task,
    x,
    y,
    choices,
    onpick,
    onclose,
  }: { task: TaskId; x: number; y: number; choices: Repo[]; onpick: (place: DoItPlace) => void; onclose: () => void } = $props();

  const items: MenuEntry[] = $derived([
    ...choices.map(
      (r): MenuEntry => ({
        id: `repo-${r.id}`,
        label: `Start in ${r.name}`,
        icon: createRawSnippet(() => ({ render: () => `<span style="display:flex;width:14px;justify-content:center">${dotHtml(r.id)}</span>` })),
        run: () => onpick({ repo: r.id }),
      }),
    ),
    'separator',
    {
      id: 'no-repo',
      label: 'No repo: research only',
      icon: createRawSnippet(() => ({ render: () => `<span style="display:flex">${iconSvg(ICONS.search)}</span>` })),
      run: () => onpick('no_repo'),
    },
  ]);
</script>

<Menu {items} {x} {y} width={240} label="Start #{task} in" {onclose} />
