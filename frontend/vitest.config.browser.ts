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
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser: 'chromium' }],
    },
  },
})
