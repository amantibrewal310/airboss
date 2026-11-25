<script lang="ts">
  import Modal from '../../../lib/components/Modal.svelte';
  import Kbd from '../../../lib/components/Kbd.svelte';
  import { useApp } from '../../../lib/state/context';
  import { sheetRows } from '../../../lib/logic/commands';

  /** ?: every shortcut, from the command registry, in two columns. Esc or ? closes. */
  const app = useApp();
  const keys = $derived(app.settings.hotkey.replace(/Control/g, 'ctrl').replace(/Alt/g, 'alt').replace(/Shift/g, 'shift').replace(/Command/g, 'mod').toLowerCase());
  const rows = $derived(sheetRows(keys));

  function key(e: KeyboardEvent) {
    if (e.key === '?') {
      e.preventDefault();
      app.close();
    }
  }
</script>

<Modal label="Keyboard shortcuts" top="8%" width={760} onclose={() => app.close()}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="sheet" role="document" tabindex="-1" data-autofocus onkeydown={key}>
    <h2>Keyboard shortcuts</h2>
    <dl class="cols">
      {#each rows as r (r.label)}
        <div class="k"><dt>{r.label}</dt><dd><Kbd keys={r.keys} /></dd></div>
      {/each}
    </dl>
    <p class="note">Every shortcut also lives in the Task and View menus. Esc closes.</p>
  </div>
</Modal>

<style>
  .sheet {
    padding: 20px 24px 18px;
    outline: none;
    max-height: 84vh;
    overflow-y: auto;
  }
  h2 {
    font-size: var(--fs-field);
    font-weight: 600;
    line-height: 20px;
    padding-bottom: 12px;
  }
  .cols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    column-gap: 32px;
  }
  .k {
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 32px;
    border-bottom: 1px solid var(--line);
  }
  dt {
    line-height: 16px;
  }
  .note {
    padding-top: 12px;
    font-size: var(--fs-meta);
    color: var(--text-3);
  }
</style>
