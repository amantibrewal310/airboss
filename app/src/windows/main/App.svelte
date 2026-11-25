<script lang="ts">
  let { title = 'Airboss' }: { title?: string } = $props();
</script>

<main class="shell">{title}</main>

<style>
  .shell {
    height: 100%;
    display: grid;
    place-items: center;
  }
</style>
