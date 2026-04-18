import { render } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { JobStats } from '$lib/stores/runs.svelte'
import { createMockRun } from '$lib/test-utils/factories'

// Must import app.css so global rules (@keyframes pulse-border, .run-card[data-status]
// animation, [data-density=compact] selectors, reduced-motion halt) are live in
// document.styleSheets. Without this import, every computed-style assertion below
// silently passes because the selectors never match anything.
import '../../app.css'

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

  describe('AC10.4: ::before accent bar', () => {
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

  describe('AC11: halo animation and reduced motion', () => {
    it('AC11.1: InProgress card has animation-name = pulse-border', async () => {
      const run = createMockRun({ status: 'InProgress' })
      const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const card = container.querySelector('.run-card') as HTMLElement
      expect(getComputedStyle(card).animationName).toBe('pulse-border')
    })

    it('AC11.2: @keyframes pulse-border animates box-shadow via --halo-color', () => {
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

    it('AC11.3: Queued and Completed cards do NOT animate', async () => {
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

    it('AC11.4: reduced-motion rule exists for InProgress cards', () => {
      // CSS media queries are evaluated by the browser against the OS/browser
      // setting. We cannot flip that from JS in @vitest/browser's current API,
      // so we verify the rule's existence as coverage for AC11.4. Behavioural
      // proof is covered by manual verification; the presence of the rule in
      // app.css is the fix.
      const hasHaltRule = stylesContainReducedMotionHalt()
      expect(hasHaltRule).toBe(true)
    })

    it('AC11.5: --halo-color differs between dark and light modes', async () => {
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

  describe('AC13: compact density hides secondary content', () => {
    it('AC13.1: compact hides run-card-meta, run-card-progress, run-card-runner', async () => {
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

    it('AC13.2: without compact, the same elements are visible', async () => {
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

    it('AC13.3: toggling density does NOT re-mount the card DOM', async () => {
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

    it('AC13.4: compact keeps the header (name + glyph + duration) visible', async () => {
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

    it('AC13.5: global class names survive compilation and match html[data-density] selector', async () => {
      document.documentElement.dataset.density = 'compact'
      const run = createMockRun({ status: 'InProgress' })
      render(RunCard, { props: { run, jobStats: emptyJobStats } })
      await settle()

      const el = document.querySelector('html[data-density="compact"] .run-card-meta')
      expect(el).toBeTruthy()
    })

    it('AC13 (padding): compact selector actually shrinks .run-card padding', async () => {
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
