import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const __dirname = dirname(fileURLToPath(import.meta.url))

describe('app.css @theme inline shadcn bridge', () => {
  // Regression for "panel renders transparent": shadcn-svelte primitives
  // (Sheet.Content, Popover.Content, Command items, etc.) reference Tailwind
  // utilities like `bg-popover`, `bg-card`, `bg-muted`, `text-muted-foreground`.
  // In Tailwind v4 those utilities are only generated when `--color-*` lives
  // inside an `@theme {}` block. Defining only `--popover: var(--surface)` on
  // `:root` does NOT bridge — Tailwind's compiler never sees it as a theme
  // color and silently drops the utility. The fix is the
  // `@theme inline { --color-*: var(--*) }` block.
  //
  // We can't catch this in a browser test (Vitest's browser config doesn't
  // run the Tailwind plugin), so we assert the bridge in the CSS source
  // directly. If the block disappears or any required mapping is missing,
  // every shadcn-themed surface will silently render with no background.
  const css = readFileSync(resolve(__dirname, './app.css'), 'utf-8')

  it.each([
    'background',
    'foreground',
    'card',
    'card-foreground',
    'popover',
    'popover-foreground',
    'primary',
    'primary-foreground',
    'secondary',
    'secondary-foreground',
    'muted',
    'muted-foreground',
    'accent',
    'accent-foreground',
    'destructive',
    'destructive-foreground',
    'border',
    'input',
    'ring',
  ])('bridges --color-%s to var(--%s)', (name) => {
    const pattern = new RegExp(`--color-${name}\\s*:\\s*var\\(--${name}\\)`)
    expect(css).toMatch(pattern)
  })

  it('uses `@theme inline` (not plain `@theme`) so cascade-based theme switching stays live', () => {
    // `inline` keeps the resolved value as `var(--popover)` at the utility
    // site, so `[data-theme]` / `[data-mode]` cascade swaps continue to flow
    // through. Plain `@theme` would freeze the value at compile time.
    expect(css).toMatch(/@theme\s+inline\s*\{[\s\S]*?--color-popover/)
  })
})
