import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    projects: ['vitest.config.unit.ts', 'vitest.config.browser.ts'],
    // Coverage config lives at the workspace level — per-project `coverage`
    // blocks are ignored by v8 in projects mode.
    coverage: {
      provider: 'v8',
      // `*.svelte` is in the include so Tailwind-instrumented components
      // (RunCard, RunDetailPanel, KanbanBoard, etc.) are reported. v8's
      // include filter is "ONLY report files matching this glob"; without
      // `.svelte`, every Svelte component is dropped from the lcov.info
      // entirely. With `.svelte`, components without unit tests appear at
      // 0% — which is why CommandPalette is explicitly excluded below.
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
        // CommandPalette is verified comprehensively through Playwright E2E
        // (e2e/palette.test.ts: AC1.1–AC1.13 plus the Recent / submenu /
        // value-collision regressions). A vitest unit test would need a
        // full Bits UI Command.Dialog mount with a real cmdk Command.Root
        // context — practically infeasible without the test-utils wrappers
        // we already use for the leaf items, and even then the derived
        // sections, keydown handler, and tick()-await sequencing are
        // dialog-mounted behaviour. Until Playwright coverage is merged
        // into the lcov pipeline, this file would otherwise sink the
        // project ratio with a phantom 0% — none of its branches are
        // genuinely uncovered.
        'src/lib/components/CommandPalette.svelte',
      ],
    },
  },
})
