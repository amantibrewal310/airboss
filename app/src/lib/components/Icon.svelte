<script lang="ts">
  import { ICONS, type IconName } from './icons';

  let {
    name,
    size = 16,
    stroke = 1.5,
    label,
  }: { name: IconName; size?: number; stroke?: number; label?: string } = $props();
  const box = $derived(name === 'chevron' ? '0 0 12 12' : '0 0 16 16');
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox={box}
  fill="none"
  stroke="currentColor"
  stroke-width={name === 'chevron' ? 2 : stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  <path d={ICONS[name]} />
</svg>

<style>
  .icon {
    flex: none;
  }
</style>
