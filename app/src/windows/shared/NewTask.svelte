<script lang="ts">
  import { tick } from 'svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import RepoDot from '../../lib/components/RepoDot.svelte';
  import StatusIcon from '../../lib/components/StatusIcon.svelte';
  import type { CreateTaskInput, ParentOption, Repo, RepoId, TaskId, Then } from '../../lib/types';
  import { extractLinks, type Classified } from '../../lib/logic/links';
  import { acceleratorLabel } from '../../lib/logic/keymap';
  import { shapeOf } from '../../lib/logic/views';
  import { score } from '../../lib/logic/search';

  /**
   * New task, as a modal in the app and as the whole quick-add window. Title (18px), notes, then
   * property pills: Todo, Repos (multi-select), Parent. Tab opens repos and Shift-Tab parent; ↓
   * in the title moves to notes and ↑ on the notes' first line moves back. Pasting a link attaches
   * it as a pill. ↩ creates; ⌘↩ creates and starts Do it.
   */
  let {
    repos,
    parents,
    hotkey,
    parent: initialParent = null,
    initialRepos = [],
    plan = false,
    inApp = true,
    onsubmit,
    oncancel,
    onaddrepos,
  }: {
    repos: Repo[];
    parents: ParentOption[];
    hotkey: string;
    parent?: TaskId | null;
    initialRepos?: RepoId[];
    plan?: boolean;
    inApp?: boolean;
    onsubmit: (input: CreateTaskInput) => void;
    oncancel: () => void;
    onaddrepos?: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let title = $state('');
  let notes = $state('');
  // svelte-ignore state_referenced_locally
  let chosen = $state<RepoId[]>([...initialRepos]);
  // svelte-ignore state_referenced_locally
  let parent = $state<TaskId | null>(initialParent);
  let links = $state<Classified[]>([]);
  let dd = $state<'repos' | 'parent' | null>(null);
  let ddQuery = $state('');
  let ddHl = $state(0);
  let titleEl: HTMLInputElement | undefined = $state();
  let notesEl: HTMLTextAreaElement | undefined = $state();
  let ddInput: HTMLInputElement | undefined = $state();
  let card: HTMLDivElement | undefined = $state();

  /** Clears the form (quick add calls this every time the panel hides). */
  export function reset(keepText = false) {
    if (!keepText) {
      title = '';
      notes = '';
      links = [];
    }
    chosen = [];
    parent = null;
    dd = null;
    ddQuery = '';
  }

  /** Puts the cursor in the title (at the end when text was kept). */
  export function focusTitle() {
    titleEl?.focus();
    const n = titleEl?.value.length ?? 0;
    titleEl?.setSelectionRange(n, n);
  }

  export function element(): HTMLDivElement | undefined {
    return card;
  }

  type Option = { kind: 'repo'; repo: Repo } | { kind: 'add' } | { kind: 'top' } | { kind: 'parent'; p: ParentOption };

  const options = $derived.by((): Option[] => {
    const q = ddQuery.trim();
    if (dd === 'repos') {
      const list: Option[] = repos.filter((r) => !q || score(q, r.name) > 0).map((repo) => ({ kind: 'repo', repo }));
      if (inApp && onaddrepos) list.push({ kind: 'add' });
      return list;
    }
    if (dd === 'parent') {
      const list: Option[] = parents
        .filter((p) => !q || score(q, `${p.id} ${p.title}`) > 0)
        .slice(0, 50)
        .map((p) => ({ kind: 'parent', p }));
      if (!q) list.unshift({ kind: 'top' });
      return list;
    }
    return [];
  });

  const parentRow = $derived(parents.find((p) => p.id === parent) ?? null);
  const repoNames = $derived(chosen.map((id) => repos.find((r) => r.id === id)?.name).filter(Boolean).join(', '));
  const empty = $derived(!title.trim());

  async function openDd(which: 'repos' | 'parent') {
    dd = dd === which ? null : which;
    ddQuery = '';
    ddHl = 0;
    await tick();
    if (dd) ddInput?.focus();
    else focusTitle();
  }

  function closeDd() {
    dd = null;
    focusTitle();
  }

  function pick(o: Option | undefined) {
    if (!o) return;
    if (o.kind === 'repo') {
      chosen = chosen.includes(o.repo.id) ? chosen.filter((x) => x !== o.repo.id) : [...chosen, o.repo.id];
    } else if (o.kind === 'add') {
      dd = null;
      onaddrepos?.();
    } else {
      parent = o.kind === 'top' ? null : o.p.id;
      closeDd();
    }
  }

  function submit(then: Then) {
    if (empty) {
      focusTitle();
      return;
    }
    onsubmit({
      title: title.trim(),
      body: notes.trim() ? notes : null,
      parent,
      repos: chosen,
      links: links.map((l) => ({ text: l.url ?? l.ref, title: null })),
      then,
    });
  }

  function fieldKey(e: KeyboardEvent, field: 'title' | 'notes') {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      oncancel();
    } else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      e.stopPropagation();
      submit(e.shiftKey ? 'plan_it' : 'do_it');
    } else if (e.key === 'Enter' && field === 'title') {
      e.preventDefault();
      e.stopPropagation();
      submit(plan ? 'plan_it' : 'none');
    } else if (e.key === 'Tab') {
      e.preventDefault();
      e.stopPropagation();
      void openDd(e.shiftKey ? 'parent' : 'repos');
    } else if (e.key === 'ArrowDown' && field === 'title' && !e.shiftKey) {
      e.preventDefault();
      notesEl?.focus();
      notesEl?.setSelectionRange(0, 0);
    } else if (e.key === 'ArrowUp' && field === 'notes' && notesEl) {
      const firstLine = !notesEl.value.slice(0, notesEl.selectionStart).includes('\n');
      if (firstLine) {
        e.preventDefault();
        focusTitle();
      }
    }
  }

  function ddKey(e: KeyboardEvent) {
    e.stopPropagation();
    const n = options.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (n) ddHl = (ddHl + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (e.metaKey || e.ctrlKey) return submit('do_it');
      pick(options[ddHl]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      closeDd();
    } else if (e.key === 'Tab') {
      e.preventDefault();
      void openDd(e.shiftKey ? 'parent' : 'repos');
    }
  }

  function paste(e: ClipboardEvent, field: 'title' | 'notes') {
    const text = e.clipboardData?.getData('text/plain') ?? '';
    const { links: found, rest } = extractLinks(text);
    if (!found.length) return;
    e.preventDefault();
    links = [...links, ...found.filter((f) => !links.some((l) => l.ref === f.ref))];
    const el = field === 'title' ? titleEl : notesEl;
    if (el && rest) {
      const start = el.selectionStart ?? el.value.length;
      const end = el.selectionEnd ?? start;
      const insert = field === 'title' ? rest.replace(/\s+/g, ' ') : rest;
      const next = el.value.slice(0, start) + insert + el.value.slice(end);
      if (field === 'title') title = next;
      else notes = next;
    }
  }

  const linkIcon = (k: Classified['kind']) => (k === 'slack' ? 'slack' : k === 'pr' ? 'pr' : k === 't3_thread' ? 'thread' : k === 'github_issue' ? 'failed' : 'doc') as 'slack';
</script>

<div class="qa" bind:this={card}>
  <div class="top">
    <span class="app"><Icon name="check" size={11} stroke={1.8} />Airboss</span>
    <span class="sep">›</span>
    <span>{plan ? 'New plan' : 'New task'}</span>
    <span class="sp"></span>
    <span class="hk">{acceleratorLabel(hotkey)} from any app</span>
  </div>
  <input
    bind:this={titleEl}
    bind:value={title}
    class="ttl"
    data-autofocus
    placeholder={plan ? 'What should the plan achieve?' : 'Task title'}
    autocomplete="off"
    spellcheck="false"
    aria-label="Title"
    onkeydown={(e) => fieldKey(e, 'title')}
    onpaste={(e) => paste(e, 'title')}
  />
  <textarea
    bind:this={notesEl}
    bind:value={notes}
    class="desc"
    placeholder="Add notes, or paste a Slack link…"
    aria-label="Notes"
    onkeydown={(e) => fieldKey(e, 'notes')}
    onpaste={(e) => paste(e, 'notes')}
  ></textarea>
  <div class="pills">
    <span class="pp set" title="New tasks start as Todo"><StatusIcon shape="todo" size={13} label={false} />Todo</span>
    <span class="wrap">
      <button type="button" class="pp" class:set={chosen.length > 0} class:focus={dd === 'repos'} aria-haspopup="listbox" aria-expanded={dd === 'repos'} onclick={() => openDd('repos')}>
        {#if chosen.length}
          <span class="dots">{#each chosen as id (id)}<RepoDot {id} />{/each}</span>{repoNames}
        {:else}
          <Icon name="repo" size={13} />Repos <span class="opt">· optional</span>
        {/if}
      </button>
      {#if dd === 'repos'}{@render dropdown('Link repos…')}{/if}
    </span>
    <span class="wrap">
      <button type="button" class="pp" class:set={parent !== null} class:focus={dd === 'parent'} aria-haspopup="listbox" aria-expanded={dd === 'parent'} onclick={() => openDd('parent')}>
        <Icon name="tree" size={13} />{parentRow ? `${parentRow.id} ${parentRow.title}` : 'Parent'}
      </button>
      {#if dd === 'parent'}{@render dropdown('Parent…')}{/if}
    </span>
    {#each links as l (l.ref)}
      <span class="pp set link" title={l.url ?? l.ref}>
        <Icon name={linkIcon(l.kind)} size={13} />{l.label}
        <button type="button" class="x" aria-label="Remove {l.label}" onclick={() => (links = links.filter((x) => x !== l))}><Icon name="close" size={11} stroke={1.8} /></button>
      </span>
    {/each}
  </div>
  <div class="ft">
    <span class="h"><Kbd keys="tab" /> repos</span>
    <span class="h"><Kbd keys="shift tab" /> parent</span>
    <span class="sp"></span>
    <span class="h"><Kbd keys="mod enter" /> create and start an agent</span>
    <button type="button" class="go" disabled={empty} onclick={() => submit(plan ? 'plan_it' : 'none')}>
      {plan ? 'Create and plan' : 'Create task'} <Kbd keys="enter" onFill />
    </button>
  </div>
</div>

{#snippet dropdown(placeholder: string)}
  <div class="dd" role="listbox" aria-label={placeholder} aria-multiselectable={dd === 'repos'}>
    <input bind:this={ddInput} bind:value={ddQuery} {placeholder} autocomplete="off" spellcheck="false" aria-label={placeholder} onkeydown={ddKey} oninput={() => (ddHl = 0)} />
    <div class="opts">
      {#each options as o, i (o.kind === 'repo' ? `r${o.repo.id}` : o.kind === 'parent' ? `p${o.p.id}` : o.kind)}
        <div
          class="o"
          class:hl={i === ddHl}
          role="option"
          aria-selected={o.kind === 'repo' ? chosen.includes(o.repo.id) : o.kind === 'parent' ? parent === o.p.id : o.kind === 'top' ? parent === null : false}
          tabindex="-1"
          onpointermove={() => (ddHl = i)}
          onpointerdown={(e) => e.preventDefault()}
          onclick={() => pick(o)}
          onkeydown={() => {}}
        >
          {#if o.kind === 'repo'}
            <span class="chk">{#if chosen.includes(o.repo.id)}<Icon name="check" size={13} stroke={1.8} />{/if}</span>
            <RepoDot id={o.repo.id} />{o.repo.name}
          {:else if o.kind === 'add'}
            <span class="chk"></span><Icon name="plus" size={12} />Add repos…
          {:else if o.kind === 'top'}
            <Icon name="list" size={13} />Top level
          {:else}
            <span style:width="{o.p.depth * 10}px" class="indent"></span><StatusIcon shape={shapeOf(o.p)} size={14} label={false} /><span class="pid">{o.p.id}</span><span class="pt">{o.p.title}</span>
          {/if}
        </div>
      {:else}
        <div class="none">No match</div>
      {/each}
    </div>
  </div>
{/snippet}

<style>
  .qa {
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    border-radius: var(--r-xl);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px 0;
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
  }
  .app {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 7px;
    border: 1px solid var(--line-2);
    border-radius: var(--r-sm);
    font-size: var(--fs-label);
  }
  .sep,
  .hk {
    color: var(--text-3);
  }
  .sp {
    flex: 1;
  }
  .ttl {
    width: 100%;
    font-size: var(--fs-quick);
    font-weight: 500;
    letter-spacing: var(--track-title);
    line-height: 26px;
    padding: 10px 18px 2px;
    outline: none;
  }
  .desc {
    width: 100%;
    height: 56px;
    resize: none;
    outline: none;
    font-size: var(--fs-notes);
    line-height: 20px;
    padding: 4px 18px;
    color: var(--text-2);
  }
  .pills {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 4px 16px 14px;
  }
  .wrap {
    position: relative;
  }
  .pp {
    height: 26px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 9px;
    border-radius: var(--r-md);
    border: 1px solid var(--line-2);
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
    white-space: nowrap;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button.pp:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .pp.set {
    color: var(--text);
  }
  .pp.focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .pp :global(svg) {
    color: var(--text-2);
  }
  .opt {
    color: var(--text-3);
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .link .x {
    display: inline-flex;
    color: var(--text-3);
    margin-right: -3px;
  }
  .link .x:hover :global(svg) {
    color: var(--text);
  }
  .ft {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-top: 1px solid var(--line);
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-3);
  }
  .h {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .go {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 0 10px 0 12px;
    border-radius: var(--r-md);
    background: var(--accent-fill);
    color: var(--on-fill);
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
  }
  .go:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .go:disabled {
    opacity: 0.45;
  }
  .dd {
    position: absolute;
    top: 31px;
    left: -1px;
    z-index: 5;
    width: 240px;
    background: var(--bg-raised);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-menu);
    padding: 4px;
    animation: drop var(--dur-menu) var(--ease);
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .dd input {
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-bottom: 1px solid var(--line);
    outline: none;
    margin-bottom: 4px;
  }
  .opts {
    max-height: 260px;
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .o {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--r-sm);
    color: var(--text);
    white-space: nowrap;
    outline: none;
  }
  .o.hl {
    background: var(--bg-sel);
  }
  .chk {
    width: 14px;
    display: inline-flex;
    justify-content: center;
    color: var(--accent);
    flex: none;
  }
  .indent {
    flex: none;
  }
  .pid {
    color: var(--text-3);
    font-size: var(--fs-meta);
    font-variant-numeric: tabular-nums;
  }
  .pt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .none {
    padding: 8px;
    color: var(--text-3);
  }
</style>
