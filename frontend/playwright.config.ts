import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  // Playwright defaults to workers=1 under CI for stability. The fully-parallel
  // setting above is a no-op without explicit workers in CI; keep CI on a
  // fraction-of-cores so it actually parallelises while leaving headroom for
  // the shared Vite dev server. Local runs keep Playwright's default
  // (~half the cores).
  workers: process.env.CI ? '75%' : undefined,
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
