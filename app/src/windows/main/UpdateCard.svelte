<script lang="ts">
  import { tick } from 'svelte';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { focusables } from '../../lib/components/trap';
  import { useApp } from '../../lib/state/context';

  /**
   * SPEC-updates: the card at the foot of the sidebar, above the connections line, only while
   * there's something to say. Update and restart, What's new and Later; then the download's
   * progress, Installing… and Restarting…; or why it failed, with Try again.
   */
  const app = useApp();
  const card = $derived(app.updates.card);
  let box: HTMLElement | undefined = $state();

  /**
   * ↩ and Space press the card's buttons. In the sidebar a bare Space would otherwise reach the
   * window's keymap and mark the selected task done.
   */
  function keys(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') e.stopPropagation();
  }

  /** The title around its version, so the version never breaks at its hyphen. */
  function parts(title: string, version: string): [string, string, string] {
    const at = title.indexOf(version);
    return at === -1 ? [title, '', ''] : [title.slice(0, at), version, title.slice(at + version.length)];
  }

  /** The buttons go once an install starts: focus stays on the card while it reports progress. */
  function install() {
    if (box?.contains(document.activeElement)) box.focus({ preventScroll: true });
    void app.installUpdate();
  }

  /** Later hides the card; focus moves on to the next control (the connections line), never to <body>. */
  async function later() {
    const had = !!box?.contains(document.activeElement);
    const side = box?.closest('aside');
    const next = side && box ? focusables(side).find((el) => !box!.contains(el) && !!(box!.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING)) : undefined;
    app.updates.hide();
    if (!had) return;
    await tick();
    if (next?.isConnected) next.focus({ preventScroll: true });
    else app.focusHome();
  }
</script>

<div class="live" aria-live="polite">
  {#if card}
    {@const [before, version, after] = parts(card.title, card.version)}
    <div class="upd" class:failed={card.kind === 'failed'} role="group" aria-labelledby="update-title" tabindex="-1" bind:this={box} data-update={card.kind}>
      <div class="head">
        <span class="ic">{#if card.kind === 'failed'}<Icon name="warn" size={14} />{:else}<Icon name="spark" size={14} />{/if}</span>
        <p class="t" id="update-title">{before}{#if version}<span class="v">{version}</span>{/if}{after}</p>
      </div>
      {#if card.kind === 'downloading'}
        <div class="prog">
          <div
            class="bar"
            class:busy={card.percent === null}
            role="progressbar"
            aria-label="Downloading Airboss {card.version}"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={card.percent ?? undefined}
          >
            <i style:width={card.percent === null ? null : `${card.percent}%`}></i>
          </div>
          {#if card.percent !== null}<span class="pc" aria-hidden="true">{card.percent}%</span>{/if}
        </div>
      {:else if card.kind === 'failed'}
        <p class="msg">{card.message}</p>
      {/if}
      {#if card.kind === 'available'}
        {@const notes = card.notesUrl}
        <div class="acts">
          <Button variant="primary" size="sm" onkeydown={keys} onclick={install}>Update and restart</Button>
          <Button variant="ghost" size="sm" onkeydown={keys} onclick={() => app.openReleaseNotes(notes)}>What's new</Button>
          <Button variant="ghost" size="sm" onkeydown={keys} onclick={later}>Later</Button>
        </div>
      {:else if card.kind === 'failed'}
        <div class="acts">
          <Button size="sm" onkeydown={keys} onclick={install}>Try again</Button>
          <Button variant="ghost" size="sm" onkeydown={keys} onclick={later}>Later</Button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Pinned with the footer: the nav list above never moves when the card comes or goes. */
  .live {
    flex: none;
  }
  .upd {
    margin: 0 0 8px;
    padding: 9px 10px 10px;
    border-radius: var(--r-lg);
    background: var(--bg-raised);
    border: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    outline: none;
  }
  .upd:focus-visible {
    box-shadow: 0 0 0 2px var(--focus);
  }
  .head {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .ic {
    display: flex;
    flex: none;
    padding-top: 1px;
    color: var(--accent);
  }
  .failed .ic {
    color: var(--st-blocked);
  }
  .t {
    min-width: 0;
    font-size: var(--fs-control);
    font-weight: 500;
    line-height: 16px;
    overflow-wrap: anywhere;
  }
  .v {
    white-space: nowrap;
  }
  .msg {
    font-size: var(--fs-meta);
    line-height: 16px;
    color: var(--text-2);
    overflow-wrap: anywhere;
    padding-left: 22px;
  }
  .prog {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 22px;
  }
  .bar {
    flex: 1;
    min-width: 0;
    height: 4px;
    border-radius: 2px;
    background: var(--bg-input);
    overflow: hidden;
    position: relative;
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: var(--accent-fill);
    transition: width var(--dur-progress) var(--ease);
  }
  /* The size isn't known yet: a short segment that slides across. */
  .busy i {
    position: absolute;
    left: 0;
    width: 35%;
    animation: slide 1.2s var(--ease) infinite;
  }
  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
  /* Reduce Motion: no sliding; the whole track fills softly instead. */
  @media (prefers-reduced-motion: reduce) {
    .busy i {
      width: 100%;
      background: var(--accent-soft);
    }
  }
  .pc {
    flex: none;
    font-size: var(--fs-label);
    line-height: 14px;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
    min-width: 30px;
    text-align: right;
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding-left: 22px;
  }
</style>
