import path from 'node:path'

import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { svelteTesting } from '@testing-library/svelte/vite'
import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  // Include tailwindcss() so `import '../../app.css'` in a browser test runs
  // through Tailwind v4's @theme processing and utility-class generation.
  // Without it, app.css is served raw — :root { --foo: ... } declarations
  // load as regular CSS, but @theme inline { --color-*: ... } and
  // @import "tailwindcss" are no-ops, so utility classes (bg-input, h-8,
  // inline-flex) never apply and computed-style assertions silently fail.
  plugins: [tailwindcss(), svelte(), svelteTesting()],
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
    // Mirror the unit project's explicit worker pin under CI only — same
    // root cause: ubuntu-24.04 reports a low availableParallelism, browser
    // tests run serially through one Chromium instance, and the suite spends
    // ~30s on browser files alone. Locally, Vitest's default is fine.
    minWorkers: process.env.CI ? 2 : undefined,
    maxWorkers: process.env.CI ? 2 : undefined,
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser: 'chromium' }],
    },
  },
})
