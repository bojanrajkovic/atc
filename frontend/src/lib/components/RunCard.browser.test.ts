import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, test, vi } from 'vitest'
import type { JobStats } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRun } from '$lib/test-utils/factories'

// Must import app.css so global rules (@keyframes pulse-border, .run-card[data-status]
// animation, [data-density=compact] selectors, reduced-motion halt) are live in
// document.styleSheets. Without this import, every computed-style assertion below
// silently passes because the selectors never match anything.
import '../../app.css'

// These browser tests exercise CSS/animation/hover-peek behavior — they don't care about
// roving tabindex focus management. Stub out getRovingContext with a static no-focus context
// so RunCard mounts without requiring a provider in the test tree.
vi.mock('$lib/components/roving/context', () => ({
  getRovingContext: () => ({
    focusedRunId: null,
    initialFocusRunId: null,
    currentFocusRunId: null,
    kanbanHasFocus: false,
    setFocus: () => {},
    setKanbanHasFocus: () => {},
    restoreFocusToInitial: () => {},
  }),
  setRovingContext: () => {},
  ROVING_CONTEXT_KEY: Symbol('RovingFocusContext'),
}))

import RunCard from './RunCard.svelte'

const mockLocalStorage = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => {
      store[k] = v
    },
    removeItem: (k: string) => {
      delete store[k]
    },
    clear: () => {
      store = {}
    },
  }
})()
vi.stubGlobal('localStorage', mockLocalStorage)

const emptyJobStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

function resetDocumentAttrs(): void {
  document.documentElement.removeAttribute('data-density')
  document.documentElement.removeAttribute('data-mode')
}

async function settle(): Promise<void> {
  // Let reactivity + layout settle before reading computed styles.
  await new Promise((r) => setTimeout(r, 50))
}

describe('RunCard (browser mode)', () => {
  beforeEach(() => {
    mockLocalStorage.clear()
    resetDocumentAttrs()
  })

  afterEach(() => {
    resetDocumentAttrs()
  })

  describe('::before accent bar', () => {
    it('has width 3px, left 0, full height, background = --status-color', async () => {
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const card = container.querySelector('.run-card')
      expect(card).toBeTruthy()
      const before = getComputedStyle(card as Element, '::before')

      expect(before.width).toBe('3px')
      expect(before.left).toBe('0px')
      // Non-transparent background — --status-color resolves to an oklch() colour.
      expect(before.backgroundColor).not.toBe('')
      expect(before.backgroundColor).not.toMatch(/rgba\(0, ?0, ?0, ?0\)/)
    })
  })

  describe('halo animation and reduced motion', () => {
    it('InProgress card has animation-name = pulse-border', async () => {
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const card = container.querySelector('.run-card') as HTMLElement
      expect(getComputedStyle(card).animationName).toBe('pulse-border')
    })

    it('@keyframes pulse-border animates box-shadow via --halo-color', () => {
      const kf = findKeyframes('pulse-border')
      expect(kf).not.toBeNull()

      const shadowByKey: Record<string, string> = {}
      for (const rule of Array.from(kf!.cssRules) as CSSKeyframeRule[]) {
        shadowByKey[rule.keyText] = rule.style.boxShadow
      }

      // 0% and 100% → transparent shadow. Accept either a joint "0%, 100%" key
      // or separate "0%" and "100%" keys.
      const zero = shadowByKey['0%'] ?? shadowByKey['0%, 100%']
      const hundred = shadowByKey['100%'] ?? shadowByKey['0%, 100%']
      expect(zero).toBeDefined()
      expect(hundred).toBeDefined()
      for (const s of [zero!, hundred!]) {
        expect(s === 'none' || /transparent|rgba\(0, ?0, ?0, ?0\)/.test(s)).toBe(true)
      }

      // 50% → non-transparent shadow with 8px blur + 2px spread.
      const fifty = shadowByKey['50%']
      expect(fifty).toBeDefined()
      expect(fifty).toMatch(/8px +2px +/)
      expect(fifty).not.toMatch(/transparent|rgba\(0, ?0, ?0, ?0\)/)
    })

    it('Queued and Completed cards do NOT animate', async () => {
      for (const status of ['Queued', 'Completed'] as const) {
        const run = createMockRun({ status })
        const { container, unmount } = render(RunCard, {
          props: { run, jobStats: emptyJobStats },
        })
        await settle()

        const card = container.querySelector('.run-card') as HTMLElement
        expect(getComputedStyle(card).animationName).not.toBe('pulse-border')
        unmount()
      }
    })

    it('reduced-motion rule exists for InProgress cards', () => {
      // CSS media queries are evaluated by the browser against the OS/browser
      // setting. We cannot flip that from JS in @vitest/browser's current API,
      // so we verify the rule's existence. Behavioural proof is covered by
      // manual verification; the presence of the rule in app.css is the fix.
      const hasHaltRule = stylesContainReducedMotionHalt()
      expect(hasHaltRule).toBe(true)
    })

    it('--halo-color differs between dark and light modes', async () => {
      // Dark (default)
      resetDocumentAttrs()
      await settle()
      const darkHalo = getComputedStyle(document.documentElement)
        .getPropertyValue('--halo-color')
        .trim()
      expect(darkHalo).not.toBe('')

      // Light
      document.documentElement.dataset.mode = 'light'
      await settle()
      const lightHalo = getComputedStyle(document.documentElement)
        .getPropertyValue('--halo-color')
        .trim()
      expect(lightHalo).not.toBe('')
      // The two modes MUST resolve to different halo colors.
      expect(lightHalo).not.toBe(darkHalo)
    })
  })

  describe('compact density hides secondary content', () => {
    it('compact hides run-card-meta, run-card-progress, run-card-runner', async () => {
      document.documentElement.dataset.density = 'compact'
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, {
        props: {
          run,
          jobStats: { completed: 1, total: 2, runnerSummary: 'ubuntu-latest' },
        },
      })
      await settle()

      for (const selector of ['.run-card-meta', '.run-card-progress', '.run-card-runner']) {
        const el = container.querySelector(selector) as HTMLElement | null
        expect(el).toBeTruthy()
        expect(getComputedStyle(el as Element).display).toBe('none')
      }
    })

    it('without compact, the same elements are visible', async () => {
      resetDocumentAttrs()
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, {
        props: {
          run,
          jobStats: { completed: 1, total: 2, runnerSummary: 'ubuntu-latest' },
        },
      })
      await settle()

      for (const selector of ['.run-card-meta', '.run-card-progress', '.run-card-runner']) {
        const el = container.querySelector(selector) as HTMLElement
        expect(el).toBeTruthy()
        expect(getComputedStyle(el).display).not.toBe('none')
      }
    })

    it('toggling density does NOT re-mount the card DOM', async () => {
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const cardBefore = container.querySelector('.run-card')
      expect(cardBefore).toBeTruthy()

      document.documentElement.dataset.density = 'compact'
      await settle()

      const cardAfter = container.querySelector('.run-card')
      // Same DOM node reference — only CSS changed.
      expect(cardAfter).toBe(cardBefore)
    })

    it('compact keeps the header (name + glyph + duration) visible', async () => {
      document.documentElement.dataset.density = 'compact'
      const run = createMockRun({ status: 'InProgress', displayTitle: 'CI — main' })
      const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const header = container.querySelector('.run-card-header') as HTMLElement
      expect(header).toBeTruthy()
      expect(getComputedStyle(header).display).not.toBe('none')

      const name = container.querySelector('.run-card-name') as HTMLElement
      expect(name).toBeTruthy()
      expect(getComputedStyle(name).display).not.toBe('none')
    })

    it('global class names survive compilation and match html[data-density] selector', async () => {
      document.documentElement.dataset.density = 'compact'
      const run = createMockRun({ status: 'InProgress' })
      render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const el = document.querySelector('html[data-density="compact"] .run-card-meta')
      expect(el).toBeTruthy()
    })

    it('compact selector actually shrinks .run-card padding', async () => {
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const card = container.querySelector('.run-card') as HTMLElement
      expect(card).toBeTruthy()

      // Comfortable (default): base rule in app.css → padding: 12px 14px.
      expect(getComputedStyle(card).padding).toBe('12px 14px')

      // Compact: [data-density="compact"] .run-card override → padding: 6px 10px.
      document.documentElement.dataset.density = 'compact'
      await settle()
      expect(getComputedStyle(card).padding).toBe('6px 10px')

      // Toggle back: padding returns to comfortable values.
      resetDocumentAttrs()
      await settle()
      expect(getComputedStyle(card).padding).toBe('12px 14px')
    })
  })
})

/**
 * Helper: sets up a matchMedia mock that returns `matches: true` for the
 * hover+pointer-fine media query (indicating a pointer device, not touch).
 */
function mockMatchMediaHover(): void {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query === '(hover: hover) and (pointer: fine)',
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

/**
 * Helper: sets up a matchMedia mock that returns `matches: false` for all
 * queries — simulating a touch device.
 */
function mockMatchMediaTouch(): void {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

describe('RunCard hover-peek behavior', () => {
  beforeEach(() => {
    // Reset uiStore state so tests do not bleed into each other
    uiStore.selectedRunId = null
    uiStore.lastTriggerRunId = null
    mockLocalStorage.clear()
    resetDocumentAttrs()
    mockMatchMediaHover()
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  test('hover for less than 250 ms does NOT show popover', async () => {
    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    // Flush the $effect that sets canHover from matchMedia
    await tick()

    const article = container.querySelector('article.run-card') as HTMLElement
    article.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }))

    // Advance 200 ms — timer has not fired yet (debounce is 250 ms)
    vi.advanceTimersByTime(200)
    await tick()

    // Mouse leaves before the timer fires — clears the debounce timer
    article.dispatchEvent(new MouseEvent('mouseleave', { bubbles: true }))

    // Advance well past the debounce threshold — timer was already cancelled
    vi.advanceTimersByTime(500)
    await tick()
    await tick()

    // Popover must NOT be open — bits-ui may keep the element in DOM
    // but data-state must NOT be "open"
    expect(document.querySelector('.hover-peek-popover[data-state="open"]')).toBeNull()
  })

  test('hover for 250 ms shows popover portal-rendered to <body>', async () => {
    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    // Flush the $effect that sets canHover from matchMedia
    await tick()

    const article = container.querySelector('article.run-card') as HTMLElement
    article.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }))

    // Advance exactly 250 ms — debounce fires
    vi.advanceTimersByTime(250)
    // Let Svelte reactivity + bits-ui portal mount settle
    await tick()
    await tick()

    const popover = document.querySelector('.hover-peek-popover')
    // popover is present after 250 ms hover and is open
    expect(popover).not.toBeNull()
    expect(popover!.getAttribute('data-state')).toBe('open')
    // popover is portal-rendered directly into <body>, not nested inside the article
    expect(popover!.closest('article.run-card')).toBeNull()
    expect(popover!.parentElement?.tagName).toBe('DIV')
    // The portal ancestor chain: popover → div (floating-ui container) → body
    expect(popover!.parentElement?.parentElement).toBe(document.body)
  })

  test('mouse-leave immediately clears popover', async () => {
    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    await tick()

    const article = container.querySelector('article.run-card') as HTMLElement
    article.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }))

    vi.advanceTimersByTime(250)
    await tick()
    await tick()

    // Confirm popover is open (data-state="open") before mouseleave
    const popover = document.querySelector('.hover-peek-popover')
    expect(popover).not.toBeNull()
    expect(popover!.getAttribute('data-state')).toBe('open')

    // Mouse leaves — popoverOpen flips to false synchronously
    article.dispatchEvent(new MouseEvent('mouseleave', { bubbles: true }))
    await tick()
    await tick()

    // Popover must be in closed state immediately (no fade-out delay)
    // bits-ui keeps the element in DOM with data-state="closed"
    expect(document.querySelector('.hover-peek-popover[data-state="open"]')).toBeNull()
  })

  test('click during hover closes popover and sets selectedRunId', async () => {
    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    await tick()

    const article = container.querySelector('article.run-card') as HTMLElement
    article.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }))

    vi.advanceTimersByTime(250)
    await tick()
    await tick()

    // Confirm popover is open (data-state="open")
    expect(document.querySelector('.hover-peek-popover[data-state="open"]')).not.toBeNull()

    // Click the inner activator button
    const activator = container.querySelector('button.run-card-activate') as HTMLElement
    activator.click()
    await tick()
    await tick()

    // Popover must be in closed state — bits-ui keeps the element but sets data-state="closed"
    expect(document.querySelector('.hover-peek-popover[data-state="open"]')).toBeNull()
    // selectedRunId must be set to the run's id
    expect(uiStore.selectedRunId).toBe(run.id)
  })

  test('touch device (no hover/pointer) does NOT instantiate popover', async () => {
    // Override matchMedia to return matches: false (touch device)
    mockMatchMediaTouch()

    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    // Flush $effect — canHover will be false from the touch mock
    await tick()

    const article = container.querySelector('article.run-card') as HTMLElement
    article.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }))

    // Advance well past debounce — timer should never have been set
    vi.advanceTimersByTime(500)
    await tick()
    await tick()

    // On touch devices: {#if canHover} is false so HoverPeekPopover is NOT rendered.
    // No .hover-peek-popover element exists in the DOM at all.
    expect(document.querySelector('.hover-peek-popover')).toBeNull()
  })
})

function findKeyframes(name: string): CSSKeyframesRule | null {
  for (const sheet of Array.from(document.styleSheets)) {
    let rules: CSSRuleList
    try {
      rules = sheet.cssRules
    } catch {
      // Cross-origin stylesheets throw on cssRules access — skip.
      continue
    }
    for (const rule of Array.from(rules)) {
      if (rule instanceof CSSKeyframesRule && rule.name === name) return rule
    }
  }
  return null
}

function stylesContainReducedMotionHalt(): boolean {
  for (const sheet of Array.from(document.styleSheets)) {
    let rules: CSSRuleList
    try {
      rules = sheet.cssRules
    } catch {
      continue
    }
    for (const rule of Array.from(rules)) {
      if (!(rule instanceof CSSMediaRule)) continue
      if (!rule.conditionText.includes('prefers-reduced-motion')) continue
      for (const inner of Array.from(rule.cssRules)) {
        if (!(inner instanceof CSSStyleRule)) continue
        if (!inner.selectorText.includes('.run-card')) continue
        if (!inner.selectorText.includes('data-status="InProgress"')) continue
        // `animation: none !important` may be normalized as either the longhand
        // `animation-name: none` or the shorthand with the important flag
        // stripped from the serialized value. Inspect both + the raw cssText.
        const text = inner.cssText
        const animNameNone =
          inner.style.getPropertyValue('animation-name') === 'none' ||
          inner.style.getPropertyValue('animation') === 'none' ||
          /animation(?:-name)?\s*:\s*none/i.test(text)
        if (animNameNone) return true
      }
    }
  }
  return false
}
