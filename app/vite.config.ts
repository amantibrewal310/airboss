import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';

/**
 * Two entry pages: the main window and the quick-add panel (its own small bundle).
 * `--mode mock` swaps the Tauri transport for the in-browser mock backend at compile time,
 * so a normal build carries no mock code.
 */
export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  clearScreen: false,
  define: {
    __MOCK__: JSON.stringify(mode === 'mock'),
  },
  // In `pnpm tauri dev` the webview's errors and warnings also print in the terminal.
  server: { port: 5173, strictPort: true, forwardConsole: { unhandledErrors: true, logLevels: ['error', 'warn'] } },
  preview: { port: 4173, strictPort: true },
  build: {
    target: 'safari16',
    cssMinify: true,
    reportCompressedSize: true,
    rolldownOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        quickadd: resolve(import.meta.dirname, 'quickadd.html'),
      },
    },
  },
}));
