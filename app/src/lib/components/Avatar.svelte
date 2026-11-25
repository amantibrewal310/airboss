<script lang="ts">
  import type { Actor } from '../types';

  /** Who acted last: a 20px circle (18px in the feed) with a white initial. */
  let { actor, size = 20 }: { actor: Actor; size?: number } = $props();
  const kind = $derived(
    actor === 'me' ? 'me' : actor.startsWith('agent') ? 'agent' : actor.startsWith('sync') ? 'sync' : actor === 'plan' ? 'plan' : actor === 'launch' ? 'launch' : 'sync',
  );
  const name = $derived(
    { me: 'You', agent: 'Agent', sync: actor === 'sync:t3' ? 'T3' : 'GitHub', plan: 'Plan', launch: 'Launch' }[kind],
  );
  const initial = $derived({ me: 'Y', agent: 'A', sync: 'S', plan: 'P', launch: 'L' }[kind]);
</script>

<span class="av {kind}" style:--s="{size}px" style:font-size="{size <= 18 ? 9 : 10}px" title={name} aria-label={name} role="img">{initial}</span>

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
