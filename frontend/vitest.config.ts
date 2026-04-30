import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    projects: ['vitest.config.unit.ts', 'vitest.config.browser.ts'],
    // Coverage config lives at the workspace level — per-project `coverage`
    // blocks are ignored by v8 in projects mode.
    coverage: {
      provider: 'v8',
      include: ['src/lib/**/*.svelte', 'src/lib/**/*.svelte.ts', 'src/lib/**/*.ts'],
      exclude: [
        'src/lib/types/**',
        // Vendored shadcn-svelte primitives — pulled in via `pnpm dlx
        // shadcn-svelte add`, mostly pass-through wrappers around bits-ui.
        // Most aren't actually imported by our app code (e.g. textarea,
        // input-group, dialog-footer); the v8 instrumenter picks them up
        // anyway and they drag the coverage ratio down for no signal.
        'src/lib/components/ui/**',
        // Test harness wrappers and factory helpers — not production code.
        // The Svelte wrappers exist solely to satisfy testing-library
        // limitations (Svelte 5 Snippets, Bits UI Command context); the
        // factories are mock-data builders for tests.
        'src/lib/components/test-utils/**',
        'src/lib/test-utils/**',
      ],
    },
  },
})
