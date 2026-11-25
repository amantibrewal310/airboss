<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import { api } from '../../lib/api';
  import type { AppError } from '../../lib/types';

  /**
   * The database won't open at start: a full-window state with the path, the reason, Show in
   * Finder and Try again. `lost`: it broke while the app ran (the live WAL deleted, an I/O or
   * corruption error), so the fix is to quit and reopen, and Try again can't help.
   */
  let { error, lost = false }: { error: AppError; lost?: boolean } = $props();
  /** `detail` is "<path>: <reason>"; the reason may have a slash of its own ("disk I/O error"). */
  const folder = $derived((error.detail ?? '').split(': ')[0]!.replace(/\/[^/]*$/, ''));
</script>

<div class="db" role="alert">
  <div class="col">
    <svg width="24" height="24" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.25" fill="none" stroke="var(--st-blocked)" stroke-width="1.4" /><path d="M8 4.8v3.6" stroke="var(--st-blocked)" stroke-width="1.4" stroke-linecap="round" /><circle cx="8" cy="10.9" r="0.85" fill="var(--st-blocked)" /></svg>
    <h1>{lost ? "Airboss's database stopped working" : "Airboss can't open its database"}</h1>
    {#if error.detail}<code>{error.detail}</code>{/if}
    <p>{error.message}</p>
    <div class="btns">
      <Button onclick={() => api.openUrl({ url: `file://${folder}` }).catch(() => {})}>Show in Finder</Button>
      {#if !lost}<Button variant="primary" onclick={() => location.reload()}>Try again</Button>{/if}
    </div>
  </div>
</div>

<style>
  .db {
    height: 100%;
    display: grid;
    place-items: center;
    background: var(--bg);
    padding: 24px;
  }
  .col {
    width: 480px;
    max-width: 100%;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h1 {
    font-size: var(--fs-headline);
    font-weight: 600;
    letter-spacing: var(--track-display);
    line-height: 26px;
    margin-top: 6px;
  }
  code {
    font-family: var(--mono);
    font-size: var(--fs-meta);
    color: var(--text);
    padding: 6px 8px;
    background: var(--bg-input);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  p {
    color: var(--text-2);
  }
  .btns {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }
</style>
