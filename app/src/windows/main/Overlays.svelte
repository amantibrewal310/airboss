<script lang="ts">
  import { useApp } from '../../lib/state/context';
  import Jump from './Jump.svelte';
  import StatusMenu from './overlays/StatusMenu.svelte';
  import ContextMenu from './overlays/ContextMenu.svelte';
  import StartInMenu from './overlays/StartInMenu.svelte';
  import RepoChecklist from './overlays/RepoChecklist.svelte';
  import BlockedReason from './overlays/BlockedReason.svelte';
  import DropConfirm from './overlays/DropConfirm.svelte';
  import LinkPopover from './overlays/LinkPopover.svelte';
  import NewTaskModal from './overlays/NewTaskModal.svelte';
  import ShortcutSheet from './overlays/ShortcutSheet.svelte';
  import Settings from './overlays/Settings.svelte';
  import AddReposModal from './overlays/AddReposModal.svelte';

  /** The one open overlay. ⌘K and ⌘N replace it; Esc closes it. */
  const app = useApp();
  const o = $derived(app.ui.overlay);
</script>

{#if o}
  {#key o}
    {#if o.kind === 'jump'}
      <Jump mode={o.mode} task={o.mode === 'jump' ? null : o.task} />
    {:else if o.kind === 'status'}
      <StatusMenu task={o.task} at={o.at} />
    {:else if o.kind === 'context'}
      <ContextMenu task={o.task} at={o.at} />
    {:else if o.kind === 'start_in'}
      <StartInMenu
        task={o.task}
        x={Math.max(8, o.at.x + (o.at.w ?? 0) - 240)}
        y={o.at.y}
        choices={o.choices}
        onclose={() => app.close()}
        onpick={(place) => {
          app.close();
          void app.doIt(o.task, place);
        }}
      />
    {:else if o.kind === 'repos'}
      <RepoChecklist task={o.task} at={o.at} />
    {:else if o.kind === 'blocked'}
      <BlockedReason task={o.task} at={o.at} />
    {:else if o.kind === 'drop'}
      <DropConfirm task={o.task} open={o.open} />
    {:else if o.kind === 'link'}
      <LinkPopover task={o.task} at={o.at} />
    {:else if o.kind === 'new'}
      <NewTaskModal parent={o.parent} repos={o.repos} plan={o.plan} />
    {:else if o.kind === 'sheet'}
      <ShortcutSheet />
    {:else if o.kind === 'settings'}
      <Settings />
    {:else if o.kind === 'add_repos'}
      <AddReposModal />
    {/if}
  {/key}
{/if}
