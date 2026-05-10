import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { defineCoverageReporterConfig } from '@bgotink/playwright-coverage'
import { defineConfig } from '@playwright/test'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// Vite + svelte's served sourcemaps reduce every entry's `sources` field to a
// basename (`CommandPalette.svelte`) instead of the full src/-relative path
// (`src/lib/components/CommandPalette.svelte`). @bgotink/playwright-coverage
// then emits lcov SF: lines using that basename, which doesn't match Vitest's
// SF: lines and breaks Codecov's server-side merge.
//
// Resolve each basename to its actual on-disk path by walking `src/` once at
// config-load time. Unique basenames in this project (enforced by the
// "one component per file" naming convention) make this a clean lookup.
const SOURCE_BY_BASENAME = (() => {
  const map = new Map<string, string>()
  const stack = [path.join(__dirname, 'src')]
  while (stack.length > 0) {
    const dir = stack.pop()!
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name)
      if (entry.isDirectory()) {
        stack.push(full)
      } else if (
        // First match wins; basenames are expected unique. Duplicates would
        // indicate a naming collision worth fixing at the source.
        (entry.name.endsWith('.svelte') ||
          entry.name.endsWith('.svelte.ts') ||
          entry.name.endsWith('.ts')) &&
        !map.has(entry.name)
      ) {
        map.set(entry.name, full)
      }
    }
  }
  return map
})()

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
  // `@bgotink/playwright-coverage` runs alongside the default list reporter.
  // Tests must import `test`/`expect` from `e2e/lib/fixtures.ts` for the
  // V8 capture hook to fire — `@playwright/test`'s own `test` skips it. The
  // reporter merges per-test V8 attachments into istanbul-format lcov at
  // `coverage/e2e/lcov.info`; CI then uploads that file alongside Vitest's
  // `coverage/vitest/lcov.info` and Codecov merges the two server-side
  // (matching by `SF:` paths).
  reporter: [
    ['list'],
    [
      '@bgotink/playwright-coverage',
      defineCoverageReporterConfig({
        sourceRoot: __dirname,
        // Absolute path. Playwright's `rootDir` is the resolved `testDir`
        // (= `frontend/e2e/`), so a relative `coverage/e2e` would land at
        // `frontend/e2e/coverage/e2e/` — pin to `frontend/coverage/e2e/`
        // alongside Vitest's output.
        resultDir: path.join(__dirname, 'coverage/e2e'),
        // Excludes are matched against the basename-only relativePath that
        // sanitizePath() produces (see `node_modules/@bgotink/playwright-
        // coverage/lib/data.js`). Pattern-glob exclusions on full paths
        // (`src/lib/components/ui/**`) don't match — basename matching is
        // the only supported shape here. We instead let the rewritePath
        // resolution determine which files end up in the lcov: anything
        // outside `src/` doesn't resolve and ends up with an unstable SF:
        // line that Codecov drops as unmatched.
        exclude: [],
        reports: [['lcovonly', { file: 'lcov.info' }], ['text-summary']],
        rewritePath: ({ relativePath, absolutePath }) => {
          const resolved = SOURCE_BY_BASENAME.get(relativePath)
          return resolved ?? absolutePath
        },
      }),
    ],
  ],
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
