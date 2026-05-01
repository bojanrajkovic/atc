import path from 'node:path'

import { svelte } from '@sveltejs/vite-plugin-svelte'
import { svelteTesting } from '@testing-library/svelte/vite'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  resolve: {
    alias: {
      $lib: path.resolve('./src/lib'),
    },
  },
  test: {
    name: 'unit',
    environment: 'jsdom',
    setupFiles: ['./vitest.setup.unit.ts'],
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.browser.test.ts', 'e2e/**'],
    // Coverage config lives in vitest.config.ts at the workspace level.
    // Per-project coverage blocks are ignored when running via the
    // workspace's `projects:` array.
  },
})
