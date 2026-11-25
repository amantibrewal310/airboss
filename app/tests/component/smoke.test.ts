import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import App from '../../src/windows/main/App.svelte';

describe('toolchain', () => {
  it('renders a Svelte component in WebKit', async () => {
    const screen = await render(App, { title: 'Hello' });
    await expect.element(screen.getByText('Hello')).toBeInTheDocument();
  });
});
