<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import { useApp } from '../../lib/state/context';
  import { onMount } from 'svelte';
  import { outsideClick } from '../../lib/components/trap';

  /** The footer: "All connected", or what's off in plain words; the popover gives the fix. */
  const app = useApp();
  const open = $derived(app.ui.overlay?.kind === 'connections');
  let root: HTMLElement | undefined = $state();

  onMount(() => outsideClick(() => root, () => open && app.close()));

  function toggle() {
    if (open) app.close();
    else app.open({ kind: 'connections' });
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) {
      e.preventDefault();
      e.stopPropagation();
      app.close();
    }
  }
</script>

<div class="conn" bind:this={root} onkeydown={keydown} role="presentation">
  {#if open}
    <div class="pop" role="dialog" aria-label="Connections">
      {#each app.conn.merged as c (c.name)}
        <div class="ci" class:off={!c.ok}>
          <i aria-hidden="true"></i>
          <div>
            <b>{c.name} · {c.state}</b>
            <span>{c.ok ? c.detail : (c.fix ?? c.detail)}</span>
          </div>
        </div>
      {/each}
      {#if app.conn.background}
        <div class="ci off">
          <i aria-hidden="true"></i>
          <div>
            <b>{app.conn.background}</b>
            <span>Quit Airboss and open it again.</span>
          </div>
        </div>
      {/if}
      <div class="foot"><Button size="sm" onclick={() => app.recheck()} data-autofocus>Check again</Button></div>
    </div>
  {/if}
  <button type="button" class="btn" class:warn={!app.conn.footer.ok} aria-expanded={open} aria-haspopup="dialog" onclick={toggle}>
    <i aria-hidden="true"></i>{app.conn.footer.text}
  </button>
</div>

<style>
  /* The hairline stops at the sidebar's 8px padding on both sides. */
  .conn {
    margin: 0;
    padding: 8px 0 10px;
    border-top: 1px solid var(--line);
    position: relative;
    flex: none;
  }
  .btn {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    width: 100%;
    padding: 0 8px;
    border-radius: var(--r-md);
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-3);
  }
  .btn:hover {
    background: var(--bg-hover);
    color: var(--text-2);
  }
  .btn i,
  .ci i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--st-review);
    flex: none;
  }
  .warn {
    color: var(--warn-text);
  }
  .warn:hover {
    color: var(--warn-text);
  }
  .warn i,
  .ci.off i {
    background: var(--st-blocked);
  }
  .pop {
    position: absolute;
    left: 8px;
    bottom: 44px;
    width: 300px;
    background: var(--bg-raised);
    border-radius: var(--r-pop);
    box-shadow: var(--shadow-menu);
    padding: 6px;
    z-index: 20;
    animation: drop var(--dur-menu) var(--ease);
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .ci {
    display: flex;
    gap: 10px;
    padding: 8px;
    border-radius: var(--r-md);
  }
  .ci i {
    margin-top: 6px;
  }
  .ci b {
    display: block;
    font-weight: 500;
    font-size: var(--fs-control);
    line-height: 18px;
  }
  .ci span {
    display: block;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
    margin-top: 1px;
  }
  .foot {
    padding: 4px 8px 6px;
    display: flex;
    justify-content: flex-end;
  }
</style>
