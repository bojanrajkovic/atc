import path from 'node:path'

import { svelte } from '@sveltejs/vite-plugin-svelte'
import { svelteTesting } from '@testing-library/svelte/vite'
import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  resolve: {
    conditions: ['browser', 'import'],
    alias: {
      $lib: path.resolve('./src/lib'),
    },
  },
  test: {
    name: 'browser',
    include: ['src/**/*.browser.test.ts'],
    exclude: ['e2e/**'],
    // Mirror the unit project's explicit worker pin — same root cause:
    // ubuntu-24.04 reports a low availableParallelism, browser tests run
    // serially through one Chromium instance, and the suite spends ~30s
    // on browser files alone.
    minWorkers: 2,
    maxWorkers: 2,
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser: 'chromium' }],
    },
  },
})
