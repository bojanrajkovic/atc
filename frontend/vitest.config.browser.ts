import path from 'node:path'

import { svelte } from '@sveltejs/vite-plugin-svelte'
import { svelteTesting } from '@testing-library/svelte/vite'
import * as browserPlaywright from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  resolve: {
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
      provider: browserPlaywright.playwright,
      instances: [{ browser: 'chromium' }],
    },
  },
})
