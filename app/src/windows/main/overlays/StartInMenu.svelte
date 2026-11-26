<script lang="ts">
  import { createRawSnippet, onMount } from 'svelte';
  import Menu, { type MenuEntry } from '../../../lib/components/Menu.svelte';
  import type { DoItPlace, Repo, T3ProjectRow, TaskId } from '../../../lib/types';
  import { ICONS } from '../../../lib/components/icons';
  import { mentionedRepos } from '../../../lib/logic/repos';
  import { useApp } from '../../../lib/state/context';
  import { dotHtml, iconSvg } from './shapes';

  /**
   * Do it on a task with no repo: "Add <name> and start" for a folder the task's title or notes
   * name that isn't a repo here yet, "Start in <repo>" for each repo (this links the repo, then
   * starts), "Add another repo…" and "No repo: research only". ↑↓ and ↩ pick; Esc closes.
   */
  let {
    task,
    x,
    y,
    choices,
    onpick,
    onclose,
  }: { task: TaskId; x: number; y: number; choices: Repo[]; onpick: (place: DoItPlace) => void; onclose: () => void } = $props();

  const app = useApp();
  let folders = $state<T3ProjectRow[]>([]);
  onMount(async () => {
    folders = app.conn.t3Ready ? await app.t3Projects() : await app.herdrFolders();
  });
  const text = $derived.by(() => {
    const d = app.detail.detail;
    return [app.row(task)?.title ?? '', d?.task.id === task ? (d.task.body ?? '') : ''].join('\n');
  });
  const mentioned = $derived(mentionedRepos(folders, text));
  const icon = (svg: string) => createRawSnippet(() => ({ render: () => `<span style="display:flex">${svg}</span>` }));

  const items: MenuEntry[] = $derived([
    ...mentioned.map(
      (f): MenuEntry => ({
        id: `add-${f.path}`,
        label: `Add ${f.name} and start`,
        icon: icon(iconSvg(ICONS.plus)),
        run: () => {
          onclose();
          void app.addRepoAndStart(task, f.path);
        },
      }),
    ),
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
      id: 'add-repo',
      label: 'Add another repo…',
      icon: icon(iconSvg(ICONS.plus)),
      run: () => app.open({ kind: 'add_repos' }),
    },
    {
      id: 'no-repo',
      label: 'No repo: research only',
      icon: createRawSnippet(() => ({ render: () => `<span style="display:flex">${iconSvg(ICONS.search)}</span>` })),
      run: () => onpick('no_repo'),
    },
  ]);
</script>

<Menu {items} {x} {y} width={240} label="Start #{task} in" {onclose} />
