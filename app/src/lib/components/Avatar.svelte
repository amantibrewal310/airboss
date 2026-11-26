<script lang="ts">
  import type { Actor, Source } from '../types';
  import { launchName } from '../logic/activity';

  /**
   * Who acted last: a 20px circle (18px in the feed) with a white initial. Launch (Do it, Plan it)
   * shows where its thread runs when `launchedIn` is known: T for T3, H for Herdr.
   */
  let { actor, launchedIn = null, size = 20, decorative = false }: { actor: Actor; launchedIn?: Source | null; size?: number; decorative?: boolean } = $props();
  const kind = $derived(
    actor === 'me' ? 'me' : actor.startsWith('agent') ? 'agent' : actor.startsWith('sync') ? 'sync' : actor === 'plan' ? 'plan' : actor === 'launch' ? 'launch' : 'sync',
  );
  const name = $derived(
    { me: 'You', agent: 'Agent', sync: actor === 'sync:t3' ? 'T3' : 'GitHub', plan: 'Plan', launch: launchName(launchedIn) }[kind],
  );
  const initial = $derived({ me: 'Y', agent: 'A', sync: 'S', plan: 'P', launch: launchedIn === 'herdr' ? 'H' : launchedIn === 't3' ? 'T' : 'L' }[kind]);
</script>

<span class="av {kind}" style:--s="{size}px" style:font-size="{size <= 18 ? 9 : 10}px" title={name} aria-label={decorative ? undefined : name} role={decorative ? undefined : 'img'} aria-hidden={decorative ? 'true' : undefined}>{initial}</span>

<style>
  .av {
    width: var(--s);
    height: var(--s);
    border-radius: 50%;
    flex: none;
    display: inline-grid;
    place-items: center;
    font-weight: 600;
    line-height: 1;
    color: var(--on-fill);
    letter-spacing: 0;
  }
  .me {
    background: var(--actor-me);
  }
  .agent {
    background: var(--actor-agent);
  }
  .sync {
    background: var(--actor-sync);
  }
  .plan {
    background: var(--actor-plan);
  }
  .launch {
    background: var(--actor-launch);
  }
</style>
