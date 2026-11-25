<script lang="ts">
  import type { Component } from 'svelte';
  import type { App } from '../../src/lib/state/app.svelte';
  import { provideApp } from '../../src/lib/state/context';

  /** Renders a main-window component with the controller in context. */
  let { app, component, props = {} }: { app: App; component: Component<any>; props?: Record<string, unknown> } = $props();
  // svelte-ignore state_referenced_locally
  provideApp(app);
  const Comp = $derived(component);
</script>

<div style="height: 900px; display: flex; position: relative;">
  <Comp {...props} />
</div>
