import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  // Playwright defaults to workers=1 under CI for stability. The fully-parallel
  // setting above is a no-op without explicit workers in CI; pin to 2 on
  // ubuntu-24.04 so it parallelises while leaving the shared Vite dev server
  // and one Chromium per worker headroom. 3 workers tripped a global-stub race
  // in run-detail-panel.test.ts (window.__scrollIntoViewCalled clobbered by
  // a sibling worker), so 2 is the empirically-safe ceiling. Local runs keep
  // Playwright's default (~half the cores).
  workers: process.env.CI ? 2 : undefined,
  timeout: 30_000,
  expect: {
    timeout: 5_000,
  },
  use: {
    baseURL: 'http://localhost:5173',
    headless: true,
  },
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' },
    },
  ],
  webServer: {
    command: 'pnpm dev',
    port: 5173,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
})
