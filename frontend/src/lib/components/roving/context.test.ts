import { render } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { RovingFocusContext } from './context'
import ContextTestHarnessCombined from './context-test-harness-combined.svelte'
import ContextTestHarnessGet from './context-test-harness-get.svelte'
import ContextTestHarnessOtherSymbol from './context-test-harness-other-symbol.svelte'

type GetResult = { ok: true; value: RovingFocusContext } | { ok: false; error: Error }

/**
 * Build a fully-specified mock RovingFocusContext with all eight members.
 * The setters are stubs — this layer only tests the store/retrieve protocol,
 * not the behavior of the setters or restorers themselves.
 */
function makeMockContext(): RovingFocusContext {
  return {
    focusedRunId: null,
    initialFocusRunId: null,
    currentFocusRunId: null,
    kanbanHasFocus: false,
    getVisibleColumns: () => [[], [], []] as const,
    setFocus: () => {},
    setKanbanHasFocus: () => {},
    restoreFocusToInitial: () => Promise.resolve(),
  }
}

describe('roving context', () => {
  describe('getRovingContext without a prior setRovingContext', () => {
    it('throws with a message mentioning RovingFocusProvider', () => {
      let result: GetResult | undefined

      render(ContextTestHarnessGet, {
        props: {
          onResult: (r) => {
            result = r
          },
        },
      })

      expect(result).toBeDefined()
      expect(result!.ok).toBe(false)
      expect((result as { ok: false; error: Error }).error.message).toMatch(/RovingFocusProvider/)
    })
  })

  describe('setRovingContext followed by getRovingContext in the same component tree', () => {
    it('retrieves the exact same context object reference (identity check)', () => {
      const ctx = makeMockContext()
      let result: GetResult | undefined

      render(ContextTestHarnessCombined, {
        props: {
          ctx,
          onResult: (r) => {
            result = r
          },
        },
      })

      expect(result).toBeDefined()
      expect(result!.ok).toBe(true)
      expect((result as { ok: true; value: RovingFocusContext }).value).toBe(ctx)
    })
  })

  describe('symbol-keyed isolation', () => {
    it('throws when only an unrelated symbol is registered, not ROVING_CONTEXT_KEY', () => {
      let result: GetResult | undefined

      render(ContextTestHarnessOtherSymbol, {
        props: {
          onResult: (r) => {
            result = r
          },
        },
      })

      expect(result).toBeDefined()
      expect(result!.ok).toBe(false)
      expect((result as { ok: false; error: Error }).error.message).toMatch(/RovingFocusProvider/)
    })
  })

  describe('round-trip with all eight context members', () => {
    it('retrieves a fully-specified context object with all members intact', () => {
      let setFocusCallCount = 0
      let setKanbanHasFocusCallCount = 0
      let restoreFocusCallCount = 0

      const ctx: RovingFocusContext = {
        focusedRunId: 42n,
        initialFocusRunId: 1n,
        currentFocusRunId: 42n,
        kanbanHasFocus: true,
        getVisibleColumns: () => [[], [], []] as const,
        setFocus: (_id) => {
          setFocusCallCount++
        },
        setKanbanHasFocus: (_v) => {
          setKanbanHasFocusCallCount++
        },
        restoreFocusToInitial: () => {
          restoreFocusCallCount++
          return Promise.resolve()
        },
      }

      let result: GetResult | undefined

      render(ContextTestHarnessCombined, {
        props: {
          ctx,
          onResult: (r) => {
            result = r
          },
        },
      })

      expect(result).toBeDefined()
      expect(result!.ok).toBe(true)

      const retrieved = (result as { ok: true; value: RovingFocusContext }).value

      // Identity: same object reference
      expect(retrieved).toBe(ctx)

      // All eight members are accessible on the retrieved reference
      expect(retrieved.focusedRunId).toBe(42n)
      expect(retrieved.initialFocusRunId).toBe(1n)
      expect(retrieved.currentFocusRunId).toBe(42n)
      expect(retrieved.kanbanHasFocus).toBe(true)
      expect(retrieved.getVisibleColumns()).toStrictEqual([[], [], []])

      // Methods are callable (not exercising setter behavior — just protocol)
      retrieved.setFocus(null)
      retrieved.setKanbanHasFocus(false)
      retrieved.restoreFocusToInitial()

      expect(setFocusCallCount).toBe(1)
      expect(setKanbanHasFocusCallCount).toBe(1)
      expect(restoreFocusCallCount).toBe(1)
    })
  })
})
