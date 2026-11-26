<script lang="ts">
  import Modal from '../../../lib/components/Modal.svelte';
  import Button from '../../../lib/components/Button.svelte';
  import Segmented from '../../../lib/components/Segmented.svelte';
  import SettingsRepos from './SettingsRepos.svelte';
  import { useApp } from '../../../lib/state/context';
  import { acceleratorLabel, acceleratorOf, RESERVED_HOTKEYS } from '../../../lib/logic/keymap';
  import { onMount } from 'svelte';
  import type { Appearance, HerdrAgent, NotificationPermission, StartAgentsIn } from '../../../lib/types';

  /**
   * ⌘,: the quick-add hotkey, open at login, appearance, repos, the command-line tool, agent
   * setup (with where agents start: T3 Code or Herdr) and the database's backup.
   */
  const app = useApp();
  let recording = $state(false);
  let hotkeyError = $state<string | null>(null);
  let saving = $state(false);

  async function record(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      recording = false;
      return;
    }
    const acc = acceleratorOf(e);
    if (!acc) return;
    recording = false;
    if (RESERVED_HOTKEYS[acc]) {
      hotkeyError = `${RESERVED_HOTKEYS[acc]} Pick another.`;
      return;
    }
    saving = true;
    hotkeyError = await app.saveSettings({ ...app.settings, hotkey: acc });
    saving = false;
  }

  const appearance = $derived(app.settings.appearance);

  /** What macOS allows, read on open and again when Airboss comes back from System Settings. */
  let permission = $state<NotificationPermission>('granted');
  onMount(() => {
    const read = async () => (permission = await app.notificationPermission());
    void read();
    window.addEventListener('focus', read);
    return () => window.removeEventListener('focus', read);
  });
  const blocked = $derived(app.settings.notifications && permission === 'denied');

  /** "Agent in Herdr" shows once Herdr is installed or chosen, as its connection row does. */
  const herdrShown = $derived(app.settings.start_agents_in === 'herdr' || (typeof app.conn.herdr === 'object' && 'ready' in app.conn.herdr));
  const startHint = $derived(
    app.settings.start_agents_in === 'auto' ? `T3 Code when it's there, else Herdr. Now ${app.conn.launcherName}.` : 'Where agents open when you start one on a task.',
  );
</script>

<Modal label="Settings" top="10%" width={560} onclose={() => app.close()}>
  <div class="set">
    <h2>Settings</h2>

    <section>
      <div class="row">
        <div class="lab">
          <b>Quick-add hotkey</b>
          <span>Opens New task from any app.</span>
        </div>
        <button
          type="button"
          class="rec"
          class:on={recording}
          data-autofocus
          aria-label="Quick-add hotkey: {acceleratorLabel(app.settings.hotkey)}. Press to record a new one."
          onclick={() => {
            recording = !recording;
            hotkeyError = null;
          }}
          onkeydown={record}
        >
          {recording ? 'Press the new keys…' : saving ? 'Saving…' : acceleratorLabel(app.settings.hotkey)}
        </button>
      </div>
      {#if hotkeyError}<p class="err" role="alert">{hotkeyError}</p>{/if}
    </section>

    <section>
      <div class="row">
        <div class="lab">
          <b>Open Airboss at login</b>
          <span>It starts in the menu bar, without a window.</span>
        </div>
        <button
          type="button"
          class="switch"
          role="switch"
          aria-checked={app.settings.open_at_login}
          aria-label="Open Airboss at login"
          onclick={() => app.setOpenAtLogin(!app.settings.open_at_login)}><i></i></button
        >
      </div>
    </section>

    <section>
      <div class="row">
        <div class="lab">
          <b>Notifications</b>
          <span>A banner when an agent asks something or needs an approval, a run fails, or an agent stops. Never while Airboss is in front.</span>
        </div>
        <button
          type="button"
          class="switch"
          role="switch"
          aria-checked={app.settings.notifications}
          aria-label="Notifications"
          onclick={() => app.setNotifications(!app.settings.notifications)}><i></i></button
        >
      </div>
      {#if blocked}
        <div class="blocked" role="status">
          <p>macOS has notifications turned off for Airboss. Turn on Allow notifications in System Settings › Notifications › Airboss.</p>
          <Button size="sm" onclick={() => app.openNotificationSettings()}>Open System Settings</Button>
        </div>
      {/if}
    </section>

    <section>
      <div class="row">
        <div class="lab">
          <b>Appearance</b>
          <span>System follows macOS.</span>
        </div>
        <Segmented
          label="Appearance"
          options={[
            { value: 'system', label: 'System' },
            { value: 'dark', label: 'Dark' },
            { value: 'light', label: 'Light' },
          ] as { value: Appearance; label: string }[]}
          value={appearance}
          onchange={(v) => app.saveSettings({ ...app.settings, appearance: v })}
        />
      </div>
    </section>

    <SettingsRepos />

    <section>
      <div class="row">
        <div class="lab">
          <b>Command-line tool</b>
          <span>
            {#if app.cli.installed}
              <code>{app.cli.path}</code>{app.cli.on_path ? '' : ' · ~/.local/bin isn’t on your PATH yet'}
            {:else}
              Installs <code>airboss</code> at <code>{app.cli.path}</code>.
            {/if}
          </span>
        </div>
        <Button onclick={() => app.installCli()}>{app.cli.installed ? 'Reinstall' : 'Install'}</Button>
      </div>
    </section>

    <section>
      <div class="row">
        <div class="lab">
          <b>Agent setup</b>
          <span>Agents report progress and ask questions through Airboss.</span>
        </div>
        <div class="btns">
          <Button onclick={() => app.copy('agent_note')}>Copy agent note</Button>
          <Button onclick={() => app.copy('setup_command')}>Copy setup command</Button>
        </div>
      </div>
      <div class="row opt">
        <div class="lab">
          <b>Start agents in</b>
          <span>{startHint}</span>
        </div>
        <Segmented
          label="Start agents in"
          options={[
            { value: 'auto', label: 'Automatic' },
            { value: 't3', label: 'T3 Code' },
            { value: 'herdr', label: 'Herdr' },
          ] as { value: StartAgentsIn; label: string }[]}
          value={app.settings.start_agents_in}
          onchange={(v) => app.saveSettings({ ...app.settings, start_agents_in: v })}
        />
      </div>
      {#if herdrShown}
        <div class="row opt">
          <div class="lab">
            <b>Agent in Herdr</b>
            <span>What a new Herdr tab runs.</span>
          </div>
          <Segmented
            label="Agent in Herdr"
            options={[
              { value: 'claude', label: 'Claude Code' },
              { value: 'codex', label: 'Codex' },
            ] as { value: HerdrAgent; label: string }[]}
            value={app.settings.herdr_agent}
            onchange={(v) => app.saveSettings({ ...app.settings, herdr_agent: v })}
          />
        </div>
      {/if}
      <!-- Each agent tool: Connected (its config has the airboss server), or how to connect it. -->
      <ul class="tools" aria-label="Agent tools">
        <li>
          <span class="tool">Claude Code</span>
          {#if app.conn.agents.claude}
            <span class="on">Connected</span>
          {:else}
            <span class="off">Not connected</span>
            <Button size="sm" aria-disabled={app.conn.connecting ? 'true' : undefined} onclick={() => app.connectClaude()}>
              {app.conn.connecting ? 'Connecting…' : 'Connect'}
            </Button>
          {/if}
        </li>
        <li>
          <span class="tool">Codex</span>
          {#if app.conn.codexOutdated}
            <span class="off warn">Codex setup needs HC_TERMINAL_ID, to run in Herdr. Paste it again into <code>~/.codex/config.toml</code></span>
            <Button size="sm" onclick={() => app.copy('codex_setup')}>Copy Codex setup</Button>
          {:else if app.conn.agents.codex}
            <span class="on">Connected</span>
          {:else}
            <span class="off">Paste into <code>~/.codex/config.toml</code></span>
            <Button size="sm" onclick={() => app.copy('codex_setup')}>Copy Codex setup</Button>
          {/if}
        </li>
      </ul>
      <pre>claude mcp add --scope user airboss -- ~/.local/bin/airboss mcp</pre>
    </section>

    <section>
      <div class="row">
        <div class="lab">
          <b>Database</b>
          <span>A copy of every task, safe to make while Airboss and your agents run.</span>
        </div>
        <Button onclick={() => app.backUp()}>Back up…</Button>
      </div>
    </section>

    <div class="ft"><Button variant="primary" onclick={() => app.close()}>Done</Button></div>
  </div>
</Modal>

<style>
  .set {
    padding: 20px 24px 18px;
    max-height: 80vh;
    overflow-y: auto;
  }
  h2 {
    font-size: var(--fs-field);
    font-weight: 600;
    line-height: 20px;
    padding-bottom: 6px;
  }
  section {
    padding: 14px 0;
    border-bottom: 1px solid var(--line);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .lab {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .lab b {
    font-weight: 500;
  }
  .lab span {
    color: var(--text-2);
    font-size: var(--fs-meta);
  }
  code,
  pre {
    font-family: var(--mono);
    font-size: var(--fs-label);
    color: var(--text);
  }
  pre {
    margin-top: 10px;
    padding: 8px 10px;
    border-radius: var(--r-md);
    background: var(--bg-input);
    border: 1px solid var(--line);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .btns {
    display: flex;
    gap: 6px;
  }
  .opt {
    margin-top: 12px;
  }
  .tools {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tools li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 24px;
    font-size: var(--fs-meta);
  }
  .tool {
    width: 92px;
    flex: none;
    font-weight: 500;
  }
  .tools .on,
  .tools .off {
    flex: 1;
    min-width: 0;
    color: var(--text-2);
  }
  .tools .warn {
    color: var(--warn-text);
  }
  .tools .on::before {
    content: '';
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-right: 6px;
    border-radius: 50%;
    background: var(--st-review);
    vertical-align: 1px;
  }
  .rec {
    min-width: 96px;
    height: 28px;
    padding: 0 12px;
    border-radius: var(--r-md);
    border: 1px solid var(--line-2);
    background: var(--bg-input);
    font-weight: 500;
    font-size: var(--fs-control);
    text-align: center;
  }
  .rec.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
    color: var(--text-2);
  }
  .err {
    margin-top: 8px;
    color: var(--warn-text);
    font-size: var(--fs-meta);
  }
  .blocked {
    margin-top: 10px;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .blocked p {
    flex: 1;
    color: var(--warn-text);
    font-size: var(--fs-meta);
    line-height: 16px;
  }
  .switch {
    width: 34px;
    height: 20px;
    border-radius: 10px;
    background: var(--bg-input);
    border: 1px solid var(--line-2);
    position: relative;
    flex: none;
    transition: background var(--dur-hover) var(--ease);
  }
  .switch i {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--text-2);
    transition: transform var(--dur-hover) var(--ease);
  }
  .switch[aria-checked='true'] {
    background: var(--accent-fill);
    border-color: transparent;
  }
  .switch[aria-checked='true'] i {
    transform: translateX(14px);
    background: #fff;
  }
  .ft {
    display: flex;
    justify-content: flex-end;
    padding-top: 16px;
  }
</style>
