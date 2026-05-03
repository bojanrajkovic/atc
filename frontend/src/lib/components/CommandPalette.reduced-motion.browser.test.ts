/**
 * CommandPalette.reduced-motion.browser.test.ts
 *
 * Verifies that the CommandPalette's theme submenu slide respects
 * prefers-reduced-motion. Uses vitest browser-mode (real DOM, Chromium).
 *
 * Strategy: vi.mock('svelte/motion', ...) at file scope with a flippable getter
 * so individual tests can toggle reduced-motion on/off. When reduced-motion is ON,
 * CommandPalette.svelte's `$derived(prefersReducedMotion.current ? 0 : 200)` yields
 * 0 and Svelte skips the Web Animations API call entirely (duration=0 short-circuits
 * the animate() path). When OFF, it yields 200 and element.animate() is called with
 * duration=200.
 *
 * We verify by checking `slideEl.getAnimations()`: with duration=0 Svelte bypasses
 * the animation entirely so getAnimations() returns []; with duration=200 Svelte
 * calls element.animate() and getAnimations() returns a non-empty array with a
 * 200ms effect. This directly exercises the gate — removing it breaks the OFF test.
 *
 * AC covered: frontend-1-0-polish.AC3.1, AC3.2
 */

import { cleanup, render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { prefersReducedMotion } from 'svelte/motion'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { paletteStore } from '$lib/stores/palette.svelte'
import { uiStore } from '$lib/stores/ui.svelte'

// Flippable mock: each test sets mockReducedMotion before rendering so the
// component sees the right value. vi.mock() is hoisted before any import that
// reads prefersReducedMotion.current (including CommandPalette.svelte and
// kanban-transitions.ts), so the getter is live from the first module resolution.
let mockReducedMotion = true
vi.mock('svelte/motion', () => ({
  prefersReducedMotion: {
    get current() {
      return mockReducedMotion
    },
  },
}))

// Mock stores that CommandPalette needs to read (prevents "store not found"
// errors during render; the component will see empty arrays / default state).
vi.mock('$lib/stores/runs.svelte', () => ({
  runStore: {
    queuedRuns: [],
    inProgressRuns: [],
    completedRuns: [],
    runs: new Map(),
    jobsByRunId: new Map(),
    jobStatsByRun: new Map(),
  },
}))

vi.mock('$lib/stores/runners.svelte', () => ({
  runnerStore: {
    pools: [],
  },
}))

vi.mock('$lib/stores/connection.svelte', () => ({
  connectionStore: {
    status: 'connected',
  },
}))

// Import CommandPalette AFTER the vi.mock() calls so the hoisted mocks are
// already in place when the module is first loaded.
import CommandPalette from './CommandPalette.svelte'

/**
 * Opens the palette and navigates to the theme submenu, then returns the
 * slide element. Throws if the slide element is not found — a missing element
 * means the submenu didn't render and the whole test is invalid.
 */
async function openSubmenuAndGetSlideEl(): Promise<Element> {
  render(CommandPalette)
  await tick()

  paletteStore.paletteOpen = true
  await tick()

  // Trigger the `transition:slide|local` by setting the theme submenu.
  // The slide div is the direct child of [data-slot="command-list"].
  paletteStore.subMenu = 'theme'
  await tick()
  // Allow the microtask queue to flush so Svelte applies the transition intro.
  await new Promise((r) => setTimeout(r, 0))

  const slideEl = document.querySelector('[data-slot="command-list"] > div')
  if (!slideEl) {
    throw new Error(
      'Slide element not found — theme submenu did not render. ' +
        'Check the [data-slot="command-list"] > div selector against the rendered DOM.',
    )
  }
  return slideEl
}

describe('CommandPalette reduced-motion gate', () => {
  beforeEach(() => {
    cleanup()
    // Reset mock and palette state before each test.
    mockReducedMotion = true
    paletteStore.paletteOpen = false
    paletteStore.subMenu = null
    paletteStore.setQuery('')
    uiStore.selectedRunId = null
    uiStore.activePoolFilter = null
  })

  afterEach(() => {
    cleanup()
  })

  it('AC3.1: prefersReducedMotion.current is true (mock bound before import)', () => {
    // Baseline assertion: if the mock didn't take effect, this would return false
    // and every DOM-level assertion below would be untestable.
    expect(prefersReducedMotion.current).toBe(true)
  })

  it('AC3.2: CommandPalette renders without errors under reduced-motion mock', async () => {
    // Smoke test: CommandPalette should mount without throwing even with the
    // mocked svelte/motion module. The palette dialog is hidden by default.
    expect(() => render(CommandPalette)).not.toThrow()
    await tick()
  })

  it('AC3.1 (reduced ON): theme submenu slide has no active Web Animation when duration is 0', async () => {
    // With reduced motion ON, submenuDuration = 0. Svelte's transition runtime
    // short-circuits when duration=0 and skips element.animate() entirely.
    // getAnimations() must return [] — confirming the gate set the duration to 0.
    mockReducedMotion = true

    const slideEl = await openSubmenuAndGetSlideEl()

    const animations = slideEl.getAnimations()
    // With duration=0, Svelte bypasses the Web Animations API call.
    // If the gate were removed (submenuDuration always 200), element.animate()
    // would be called and getAnimations() would return a non-empty array.
    expect(animations).toHaveLength(0)
  })

  it('AC3.1 (reduced OFF): theme submenu slide has an active 200ms Web Animation when duration is 200', async () => {
    // With reduced motion OFF, submenuDuration = 200. Svelte calls
    // element.animate(keyframes, { duration: 200 }) and the animation is
    // active immediately after the intro starts.
    mockReducedMotion = false

    const slideEl = await openSubmenuAndGetSlideEl()

    const animations = slideEl.getAnimations()
    // With duration=200, element.animate() is called and at least one animation
    // should be running. If the gate were removed (always 200), this would pass
    // trivially — but the reduced-ON test above would have caught the regression.
    expect(animations.length).toBeGreaterThan(0)

    // Confirm the animation effect duration is ~200ms (the value from the gate).
    const firstEffect = animations[0]?.effect
    if (firstEffect && 'getTiming' in firstEffect) {
      const timing = (firstEffect as AnimationEffect).getTiming()
      expect(timing.duration).toBe(200)
    }
  })
})
