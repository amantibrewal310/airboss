<script lang="ts">
  import Modal from '../../../lib/components/Modal.svelte';
  import { useApp } from '../../../lib/state/context';
  import AddRepos from '../AddRepos.svelte';

  const app = useApp();
</script>

<Modal label="Add repos" top="14%" width={560} onclose={() => app.close()}>
  <div class="m">
    <h2>Add repos</h2>
    <p>Pick the folders your tasks touch. Each repo keeps one colour everywhere.</p>
    <AddRepos ondone={() => app.close()} />
  </div>
</Modal>

<style>
  .m {
    padding: 20px 24px 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  h2 {
    font-size: var(--fs-field);
    font-weight: 600;
    line-height: 20px;
  }
  p {
    color: var(--text-2);
    margin-top: -6px;
  }
</style>
