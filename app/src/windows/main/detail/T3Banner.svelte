<script lang="ts">
  import { useApp } from '../../../lib/state/context';

  /** Under the header while T3 Code is missing: Do it and Plan it read as disabled. */
  const app = useApp();
</script>

<div class="t3" role="status">
  <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.25" fill="none" stroke="var(--st-blocked)" stroke-width="1.6" /><path d="M8 4.8v3.6" stroke="var(--st-blocked)" stroke-width="1.6" stroke-linecap="round" /><circle cx="8" cy="10.9" r="0.95" fill="var(--st-blocked)" /></svg>
  <span>Do it and Plan it need T3 Code. Install it, then choose <button type="button" onclick={() => app.recheck()}>Check again</button>.</span>
</div>

<style>
  .t3 {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 20px;
    background: var(--warn-soft);
    border-bottom: 1px solid var(--warn-edge);
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    flex: none;
  }
  span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button {
    color: var(--link);
    font-weight: 500;
  }
  button:hover {
    text-decoration: underline;
  }
</style>
