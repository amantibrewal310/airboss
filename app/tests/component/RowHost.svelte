<script lang="ts">
  import TaskRow from '../../src/windows/main/TaskRow.svelte';
  import type { Repo, RowDto } from '../../src/lib/types';

  /** A TaskRow in a list-sized container, so the narrow-width container queries apply. */
  let { row, width, repos, now, selected = false }: { row: RowDto; width: number; repos: Repo[]; now: number; selected?: boolean } = $props();
  const byId = $derived(new Map(repos.map((r) => [r.id, r])));
</script>

<div class="list" style:width="{width}px">
  <TaskRow {row} depth={row.depth} repos={byId} {now} {selected} keyboard={selected} />
</div>

<style>
  .list {
    container-type: inline-size;
    container-name: list;
  }
</style>
