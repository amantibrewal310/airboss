<script lang="ts">
  /** Grey lines while a detail loads (only after 150 ms). */
  let { lines = 6 }: { lines?: number } = $props();
</script>

<div class="sk" aria-hidden="true">
  {#each { length: lines } as _, i (i)}<i style:width="{[72, 54, 88, 40, 66, 80][i % 6]}%"></i>{/each}
</div>

<style>
  .sk {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-top: 6px;
  }
  i {
    display: block;
    height: 10px;
    border-radius: 5px;
    background: var(--bg-input);
  }
</style>
