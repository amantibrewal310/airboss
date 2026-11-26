<script lang="ts">
  import { tick } from 'svelte';
  import Button from '../../../lib/components/Button.svelte';
  import RepoDot from '../../../lib/components/RepoDot.svelte';
  import { useApp } from '../../../lib/state/context';
  import { removeQuestion, tasksLinkedTo } from '../../../lib/logic/repos';
  import type { Repo, RepoId } from '../../../lib/types';

  /**
   * Settings › Repos: each repo with its folder and remote. Edit renames it, points it at
   * another folder or changes its remote; Remove asks first, saying how many tasks lose the
   * link. ↩ saves an edit; Esc cancels an edit or a removal before it closes Settings.
   */
  const app = useApp();
  let editing = $state<RepoId | null>(null);
  let removing = $state<RepoId | null>(null);
  let name = $state('');
  let path = $state('');
  let remote = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);
  let root: HTMLElement | undefined = $state();

  const home = (p: string) => p.replace(/^\/Users\/[^/]+/, '~');

  async function focus(selector: string) {
    await tick();
    root?.querySelector<HTMLElement>(selector)?.focus();
  }

  function edit(r: Repo) {
    removing = null;
    editing = r.id;
    name = r.name;
    path = r.path;
    remote = r.remote_url ?? '';
    error = null;
    void focus(`#repo-name-${r.id}`);
  }

  function ask(r: Repo) {
    editing = null;
    error = null;
    removing = r.id;
    void focus(`[data-cancel-remove="${r.id}"]`);
  }

  /** Back to the row's own button, so the keyboard never loses its place. */
  function done(id: RepoId, button: 'edit' | 'remove') {
    editing = null;
    removing = null;
    error = null;
    void focus(`[data-${button}="${id}"]`);
  }

  async function chooseFolder() {
    const picked = await app.pickFolder();
    if (picked) path = picked;
  }

  async function save(id: RepoId) {
    if (busy) return;
    busy = true;
    error = await app.editRepo(id, name, path, remote.trim() || null);
    busy = false;
    if (!error) done(id, 'edit');
  }

  async function remove(id: RepoId) {
    if (busy) return;
    busy = true;
    const removed = await app.removeRepo(id);
    busy = false;
    if (removed) {
      removing = null;
      void focus('[data-add-repos]');
    }
  }

  /** Esc cancels the edit or the removal first; the next Esc closes Settings. */
  function cancelOnEsc(e: KeyboardEvent, id: RepoId) {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    done(id, editing === id ? 'edit' : 'remove');
  }
</script>

<section bind:this={root} aria-labelledby="settings-repos">
  <div class="row">
    <div class="lab">
      <b id="settings-repos">Repos</b>
      <span>Do it starts agents in these folders. Tasks keep a repo when it's renamed or moved.</span>
    </div>
    <Button data-add-repos onclick={() => app.open({ kind: 'add_repos' })}>Add repos…</Button>
  </div>
  {#if app.tasks.repos.length}
    <ul>
      {#each app.tasks.repos as r (r.id)}
        <li>
          <div class="repo">
            <span class="dot"><RepoDot id={r.id} /></span>
            <div class="info">
              <b>{r.name}</b>
              <code title={r.path}>{home(r.path)}</code>
              {#if r.remote_url}<span class="remote" title={r.remote_url}>{r.remote_url}</span>{/if}
            </div>
            {#if editing !== r.id && removing !== r.id}
              <div class="btns">
                <Button size="sm" variant="ghost" data-edit={r.id} aria-label="Edit {r.name}" onclick={() => edit(r)}>Edit</Button>
                <Button size="sm" variant="ghost" data-remove={r.id} aria-label="Remove {r.name}" onclick={() => ask(r)}>Remove…</Button>
              </div>
            {/if}
          </div>
          {#if editing === r.id}
            <!-- A form, so ↩ in any field saves. -->
            <form
              class="form"
              aria-label="Edit {r.name}"
              onsubmit={(e) => {
                e.preventDefault();
                void save(r.id);
              }}
            >
              <label for="repo-name-{r.id}">Name</label>
              <input id="repo-name-{r.id}" bind:value={name} autocomplete="off" spellcheck="false" onkeydown={(e) => cancelOnEsc(e, r.id)} />
              <label for="repo-path-{r.id}">Folder</label>
              <div class="folder">
                <input id="repo-path-{r.id}" class="mono" bind:value={path} autocomplete="off" spellcheck="false" onkeydown={(e) => cancelOnEsc(e, r.id)} />
                <Button size="sm" onclick={chooseFolder} onkeydown={(e: KeyboardEvent) => cancelOnEsc(e, r.id)}>Choose…</Button>
              </div>
              <label for="repo-remote-{r.id}">Remote</label>
              <input id="repo-remote-{r.id}" class="mono" bind:value={remote} placeholder="None" autocomplete="off" spellcheck="false" onkeydown={(e) => cancelOnEsc(e, r.id)} />
              {#if error}<p class="err" role="alert">{error}</p>{/if}
              <div class="actions">
                <Button size="sm" variant="ghost" onclick={() => done(r.id, 'edit')} onkeydown={(e: KeyboardEvent) => cancelOnEsc(e, r.id)}>Cancel</Button>
                <Button size="sm" variant="primary" type="submit" disabled={busy || !name.trim()} onkeydown={(e: KeyboardEvent) => cancelOnEsc(e, r.id)}>Save</Button>
              </div>
            </form>
          {:else if removing === r.id}
            <div class="confirm" role="alertdialog" aria-label="Remove {r.name}">
              <p>{removeQuestion(r.name, tasksLinkedTo(app.tasks.rows, r.id))}</p>
              <span>Its folder and its threads stay as they are.</span>
              <div class="actions">
                <Button size="sm" variant="ghost" data-cancel-remove={r.id} onclick={() => done(r.id, 'remove')} onkeydown={(e: KeyboardEvent) => cancelOnEsc(e, r.id)}>Cancel</Button>
                <Button size="sm" variant="primary" disabled={busy} onclick={() => remove(r.id)} onkeydown={(e: KeyboardEvent) => cancelOnEsc(e, r.id)}>Remove</Button>
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  section {
    padding: 14px 0;
    border-bottom: 1px solid var(--line);
  }
  .row,
  .repo {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .lab,
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .lab b,
  .info b {
    font-weight: 500;
  }
  .lab span,
  .confirm span {
    color: var(--text-2);
    font-size: var(--fs-meta);
  }
  ul {
    list-style: none;
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  li {
    padding: 8px 10px;
    border-radius: var(--r-item);
    background: var(--bg-input);
  }
  .repo {
    gap: 10px;
  }
  .dot {
    width: 12px;
    display: inline-flex;
    justify-content: center;
    align-self: flex-start;
    padding-top: 5px;
  }
  .info code,
  .remote,
  .mono {
    font-family: var(--mono);
    font-size: var(--fs-label);
  }
  .info code,
  .remote {
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btns {
    display: flex;
    gap: 2px;
  }
  .form {
    display: grid;
    grid-template-columns: 64px 1fr;
    align-items: center;
    gap: 8px 10px;
    margin: 10px 0 2px 22px;
  }
  .form label {
    font-size: var(--fs-meta);
    font-weight: 500;
    color: var(--text-2);
  }
  input {
    height: 28px;
    min-width: 0;
    width: 100%;
    padding: 0 8px;
    border-radius: var(--r-md);
    background: var(--bg-raised);
    border: 1px solid var(--line-2);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .folder {
    display: flex;
    gap: 6px;
    min-width: 0;
  }
  .err {
    grid-column: 2;
    color: var(--warn-text);
    font-size: var(--fs-meta);
  }
  .actions {
    grid-column: 1 / -1;
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 2px;
  }
  .confirm {
    margin: 8px 0 2px 22px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .confirm p {
    font-weight: 500;
  }
  .confirm .actions {
    margin-top: 8px;
  }
</style>
