import { defineConfig, devices } from '@playwright/test';

/**
 * The smoke test, the frame-time measurement and the screenshot baselines all run on the
 * mock build (`pnpm build:mock`) in WebKit, the engine inside the Mac app.
 */
export default defineConfig({
  testDir: 'tests/e2e',
  outputDir: 'tests/e2e/.results',
  snapshotPathTemplate: 'tests/e2e/baselines/{arg}{ext}',
  fullyParallel: true,
  forbidOnly: true,
  reporter: [['list']],
  expect: {
    toHaveScreenshot: { maxDiffPixelRatio: 0.002, animations: 'disabled', caret: 'hide' },
  },
  use: {
    baseURL: 'http://localhost:4173',
    viewport: { width: 1440, height: 900 },
    timezoneId: 'UTC',
    locale: 'en-GB',
  },
  projects: [{ name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 } }],
  webServer: {
    command: 'pnpm preview:mock',
    url: 'http://localhost:4173',
    // Never test someone else's build: a preview left running on 4173 by another worktree would
    // otherwise be reused silently (its tests then fail against the wrong UI).
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
