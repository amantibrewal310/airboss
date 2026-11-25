import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import Kbd from '../../src/lib/components/Kbd.svelte';

describe('toolchain', () => {
  it('renders a Svelte component in WebKit', async () => {
    const screen = await render(Kbd, { keys: 'mod+enter' });
    await expect.element(screen.getByText('⌘')).toBeInTheDocument();
    await expect.element(screen.getByText('↩')).toBeInTheDocument();
  });
});
