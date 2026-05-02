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
    pool: 'threads',
    // Pin worker count: Vitest defaults to `os.availableParallelism() - 1`,
    // which on the ubuntu-24.04 GitHub runner appears to resolve to 1
    // (cgroup-limited or similar) — the CI run was 91s wall ≈ aggregate, ie
    // essentially serial. Force 2 threads so CI actually parallelises; local
    // runs at 14 cores are throttled but the suite is already <6s so the
    // local hit is invisible.
    minWorkers: 2,
    maxWorkers: 2,
    setupFiles: ['./vitest.setup.unit.ts'],
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.browser.test.ts', 'e2e/**'],
    // Coverage config lives in vitest.config.ts at the workspace level.
    // Per-project coverage blocks are ignored when running via the
    // workspace's `projects:` array.
  },
})
