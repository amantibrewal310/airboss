<script lang="ts">
  import Segmented from '../../src/lib/components/Segmented.svelte';

  /** A Segmented control that owns its value, like Settings › Appearance. */
  let value = $state<'system' | 'dark' | 'light'>('system');
</script>

<Segmented
  label="Appearance"
  options={[
    { value: 'system', label: 'System' },
    { value: 'dark', label: 'Dark' },
    { value: 'light', label: 'Light' },
  ]}
  {value}
  onchange={(v) => (value = v)}
/>
