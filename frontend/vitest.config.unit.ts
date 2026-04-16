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
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.browser.test.ts', 'e2e/**'],
    coverage: {
      provider: 'v8',
      include: ['src/lib/**/*.svelte.ts', 'src/lib/**/*.ts'],
      exclude: ['src/lib/types/**'],
    },
  },
})
