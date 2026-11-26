<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import { useApp } from '../../lib/state/context';
  import { acceleratorLabel } from '../../lib/logic/keymap';
  import AddRepos from './AddRepos.svelte';
  import type { Connection } from '../../lib/types';

  /**
   * First run (any start with no tasks): "Nothing here yet", New task and Change hotkey…, the
   * ticked "Open Airboss at login" box, the Add repos step while no repo is added, and the
   * connections with what each needs.
   */
  const app = useApp();
  const rows = $derived(app.conn.merged.filter((c) => ['Agents', 'T3 Code', 'GitHub'].includes(c.name)));
  const order = (c: Connection) => ['Agents', 'T3 Code', 'GitHub'].indexOf(c.name);

  const agentsLabel = $derived(
    { install: 'Install command-line tool', connect: app.conn.connecting ? 'Connecting…' : 'Connect Claude Code', note: 'Copy agent note' }[app.conn.agentsStep],
  );

  function agentsAction() {
    const step = app.conn.agentsStep;
    if (step === 'install') void app.installCli();
    else if (step === 'connect') void app.connectClaude();
    else void app.copy('agent_note');
  }

  function detail(c: Connection): string {
    if (c.ok) return c.detail;
    // Connect Claude Code is right here; the fix points elsewhere.
    if (c.name === 'Agents' && app.conn.agentsStep === 'connect') return c.detail;
    return c.fix ?? c.detail;
  }
</script>

<div class="fr" bind:this={app.focusTargets.list} tabindex="-1">
  <div class="col">
    <div class="intro">
      <h1>Nothing here yet</h1>
      <p>Add your first task with ⌘N or C here, or {acceleratorLabel(app.settings.hotkey)} from anywhere on your Mac. Hand it to an agent when it's ready.</p>
      <div class="btns">
        <Button variant="primary" onclick={() => app.openNew()}><Icon name="plus" size={13} stroke={1.6} />New task <Kbd keys="mod+n" onFill joined /></Button>
        <Button onclick={() => app.open({ kind: 'settings' })}>Change hotkey…</Button>
      </div>
      <div class="ticks">
        <label class="login">
          <input type="checkbox" checked={app.settings.open_at_login} onchange={(e) => app.setOpenAtLogin((e.currentTarget as HTMLInputElement).checked)} />
          <span>Open Airboss at login</span>
        </label>
        <label class="login">
          <input type="checkbox" checked={app.settings.notifications} onchange={(e) => app.setNotifications((e.currentTarget as HTMLInputElement).checked)} />
          <span>Notify me when an agent asks something or stops</span>
        </label>
      </div>
    </div>

    {#if !app.tasks.repos.length}
      <section>
        <h2>Repos</h2>
        <AddRepos embedded />
      </section>
    {/if}

    <section>
      <h2>Connections</h2>
      <div class="conns">
        {#each [...rows].sort((a, b) => order(a) - order(b)) as c (c.name)}
          <div class="c">
            <span class="ic">
              {#if c.ok}
                <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="7" fill="var(--st-review)" /><path d="M5 8.2l2 2 4-4.3" fill="none" stroke="var(--bg)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
              {:else}
                <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.25" fill="none" stroke="var(--st-blocked)" stroke-width="1.6" /><path d="M8 4.8v3.6" stroke="var(--st-blocked)" stroke-width="1.6" stroke-linecap="round" /><circle cx="8" cy="10.9" r="0.95" fill="var(--st-blocked)" /></svg>
              {/if}
            </span>
            <div class="tx">
              <b>{c.name} · {c.state}</b>
              <span>{#each detail(c).split('`') as part, i (i)}{#if i % 2}<code>{part}</code>{:else}{part}{/if}{/each}</span>
            </div>
            {#if c.name === 'Agents'}
              <!-- One button whose step changes, so focus stays on it from Install to Connect to Copy. -->
              <Button aria-disabled={app.conn.connecting ? 'true' : undefined} onclick={agentsAction}>{agentsLabel}</Button>
            {:else}
              <Button onclick={() => app.recheck()}>Check again</Button>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  </div>
</div>

<style>
  .fr {
    flex: 1;
    overflow-y: auto;
    display: flex;
    justify-content: center;
    padding: 120px 24px 60px;
    outline: none;
  }
  .col {
    width: 560px;
    display: flex;
    flex-direction: column;
    gap: 40px;
  }
  .intro {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h1 {
    font-size: var(--fs-headline);
    font-weight: 600;
    letter-spacing: var(--track-display);
    line-height: 26px;
  }
  .intro p {
    color: var(--text-2);
    line-height: 20px;
  }
  .btns {
    display: flex;
    gap: 8px;
    padding-top: 12px;
  }
  .ticks {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
  }
  .login {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    width: fit-content;
  }
  .login input {
    width: 14px;
    height: 14px;
    accent-color: var(--accent-fill);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h2 {
    font-size: var(--fs-label);
    font-weight: 500;
    line-height: 14px;
    color: var(--text-3);
  }
  .conns {
    border: 1px solid var(--line);
    border-radius: var(--r-pop);
    background: var(--bg-raised);
  }
  .c {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 14px 14px 14px 16px;
    border-bottom: 1px solid var(--line);
  }
  .c:last-child {
    border-bottom: 0;
  }
  .ic {
    height: 20px;
    width: 16px;
    display: flex;
    align-items: center;
    flex: none;
  }
  .tx {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .tx b {
    font-weight: 500;
    line-height: 20px;
  }
  .tx span {
    color: var(--text-2);
    font-size: var(--fs-meta);
    line-height: 16px;
  }
  code {
    font-family: var(--mono);
    color: var(--text);
  }
</style>
