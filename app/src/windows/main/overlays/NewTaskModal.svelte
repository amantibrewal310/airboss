<script lang="ts">
  import Modal from '../../../lib/components/Modal.svelte';
  import NewTask from '../../shared/NewTask.svelte';
  import type { CreateTaskInput, ParentOption, RepoId, TaskId } from '../../../lib/types';
  import { useApp } from '../../../lib/state/context';
  import { isOpenStatus } from '../../../lib/logic/views';

  /** C, ⌘N or ⌥N: New task on the scrim. A parent's view presets the parent; a repo's view, the repo. */
  let { parent, repos, plan }: { parent: TaskId | null; repos: RepoId[]; plan: boolean } = $props();
  const app = useApp();
  const parents = $derived(
    app.tasks.order
      .map((id) => app.tasks.rows.get(id)!)
      .filter((r) => isOpenStatus(r.status))
      .map((r): ParentOption => ({ id: r.id, title: r.title, status: r.status, waiting: r.waiting, depth: r.depth })),
  );

  function submit(input: CreateTaskInput) {
    app.close();
    void app.create(input);
  }
</script>

<Modal label={plan ? 'New plan' : 'New task'} top="16%" width={680} clip={false} onclose={() => app.close()}>
  <NewTask
    repos={app.tasks.repos}
    {parents}
    hotkey={app.settings.hotkey}
    {parent}
    initialRepos={repos}
    {plan}
    onsubmit={submit}
    oncancel={() => app.close()}
    onaddrepos={() => app.open({ kind: 'add_repos' })}
  />
</Modal>
