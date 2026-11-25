import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { playwright } from '@vitest/browser-playwright';

/** Unit tests run in Node; component tests run in WebKit, the engine the Mac app uses. */
export default defineConfig({
  plugins: [svelte()],
  define: { __MOCK__: 'true' },
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: 'unit',
          environment: 'node',
          include: ['tests/unit/**/*.test.ts'],
        },
      },
      {
        extends: true,
        test: {
          name: 'component',
          include: ['tests/component/**/*.test.ts'],
          browser: {
            enabled: true,
            headless: true,
            provider: playwright(),
            instances: [{ browser: 'webkit' }],
            viewport: { width: 1440, height: 900 },
          },
        },
      },
    ],
  },
});
