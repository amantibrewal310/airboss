<script lang="ts">
  import { createRawSnippet } from 'svelte';
  import Menu, { type MenuEntry } from '../../../lib/components/Menu.svelte';
  import type { TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import type { Anchor } from '../../../lib/state/ui.svelte';
  import { BY_ID, CONTEXT_MENU, type CommandId } from '../../../lib/logic/commands';
  import { keyLabel } from '../../../lib/logic/keymap';
  import { isTreeLayout, shapeOf } from '../../../lib/logic/views';
  import { ICONS } from '../../../lib/components/icons';
  import { iconSvg, shapeSvg } from './shapes';

  /** Right-click a row or station: the task commands, from the registry. Commands that can't run are greyed with the reason. */
  let { task, at }: { task: TaskId; at: Anchor } = $props();
  const app = useApp();
  const row = $derived(app.row(task));

  const ICON: Partial<Record<CommandId, string>> = {
    do_it: ICONS.play,
    plan_it: ICONS.waves,
    do_it_whole: ICONS.play,
    toggle_done: ICONS.check,
    rename: ICONS.edit,
    link_repos: ICONS.repo,
    add_waits_on: ICONS.dep,
    move_up: ICONS.up,
    move_down: ICONS.down,
    open_page: ICONS.expand,
    copy_number: ICONS.link,
    drop: ICONS.close,
  };

  function reason(id: CommandId): string | false {
    if (!row) return 'Gone';
    const closed = row.status === 'done' || row.status === 'dropped';
    switch (id) {
      case 'do_it':
      case 'plan_it':
      case 'do_it_whole':
        if (!app.conn.launchReady) return app.conn.launchOff;
        if (row.status === 'proposed') return 'Accept the proposal first';
        if (id !== 'plan_it' && row.waiting) return `Waits on ${row.waits_on.join(', ')}`;
        return false;
      case 'move_up':
      case 'move_down':
        return isTreeLayout(app.ui.view, app.ui.layout) ? false : 'Reorder in All tasks or a parent’s view';
      case 'drop':
        return row.status === 'dropped' ? 'Already dropped' : false;
      case 'toggle_done':
        return row.status === 'proposed' ? 'Accept the proposal first' : false;
      case 'add_waits_on':
        return closed ? `#${task} is ${row.status}` : false;
      default:
        return false;
    }
  }

  const items: MenuEntry[] = $derived(
    // "Do it as one thread" only means something on a task with subtasks.
    CONTEXT_MENU.filter((id) => id !== 'do_it_whole' || !!row?.has_children).map((id): MenuEntry => {
      if (id === '-') return 'separator';
      const cmd = BY_ID.get(id)!;
      const label =
        id === 'toggle_done' && row?.status === 'done' ? 'Mark not done' : id === 'do_it' && row?.has_children ? `Start ready (${app.readySteps(task).length})` : cmd.label;
      const svg = id === 'status' && row ? shapeSvg(shapeOf(row), 14) : ICON[id] ? iconSvg(ICON[id]!) : '';
      return {
        id,
        label,
        hint: keyLabel(cmd.bind[0]?.[0] ?? ''),
        disabled: reason(id),
        icon: svg ? createRawSnippet(() => ({ render: () => `<span style="display:flex">${svg}</span>` })) : undefined,
        run: () => app.runCommand(id),
      };
    }),
  );
</script>

<Menu {items} x={at.x} y={at.y} width={240} label="Actions for #{task}" onclose={() => app.close()} />
