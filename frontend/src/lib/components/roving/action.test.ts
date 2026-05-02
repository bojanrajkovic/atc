import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createMockRun } from '$lib/test-utils/factories'
import { roving } from './action'
import type { RovingFocusContext } from './context'

// ---------------------------------------------------------------------------
// runStore mock — must come before any import that transitively loads the store
// ---------------------------------------------------------------------------

// Fixture arrays for the mock store. These are reassigned per-test to control
// what `columnsSnapshot()` returns.
let mockQueued = [createMockRun({ id: 100n, status: 'Queued' })]
let mockInProgress = [createMockRun({ id: 200n, status: 'InProgress' })]
let mockCompleted = [createMockRun({ id: 300n, status: 'Completed' })]

vi.mock('$lib/stores/runs.svelte', () => ({
  runStore: {
    get queuedRuns() {
      return mockQueued
    },
    get inProgressRuns() {
      return mockInProgress
    },
    get completedRuns() {
      return mockCompleted
    },
  },
}))

// ---------------------------------------------------------------------------
// Mock context factory
// ---------------------------------------------------------------------------

function makeMockContext(): RovingFocusContext & {
  _setFocusCalls: (bigint | null)[]
  _setKanbanHasFocusCalls: boolean[]
} {
  const _setFocusCalls: (bigint | null)[] = []
  const _setKanbanHasFocusCalls: boolean[] = []
  let focusedRunId: bigint | null = null
  let kanbanHasFocus = false

  return {
    get focusedRunId() {
      return focusedRunId
    },
    initialFocusRunId: null,
    get currentFocusRunId() {
      return focusedRunId
    },
    get kanbanHasFocus() {
      return kanbanHasFocus
    },
    setFocus(id) {
      focusedRunId = id
      _setFocusCalls.push(id)
    },
    setKanbanHasFocus(v) {
      kanbanHasFocus = v
      _setKanbanHasFocusCalls.push(v)
    },
    restoreFocusToInitial() {
      /* no-op for this layer */
    },
    _setFocusCalls,
    _setKanbanHasFocusCalls,
  }
}

// ---------------------------------------------------------------------------
// DOM helpers
// ---------------------------------------------------------------------------

/**
 * Build a minimal kanban subtree and append it to document.body.
 *
 * Structure:
 *   <div id="kanban">              ← action node
 *     <article data-run-id="100">  ← RunCard ancestor
 *       <button class="run-card-activate">open</button>
 *     </article>
 *     <article data-run-id="200">
 *       <button class="run-card-activate">open</button>
 *     </article>
 *     <button id="non-card">no class</button>
 *   </div>
 *   <button id="outside">outside the kanban</button>
 */
function buildDOM() {
  const kanban = document.createElement('div')
  kanban.id = 'kanban'

  const card1 = document.createElement('article')
  card1.setAttribute('data-run-id', '100')
  const btn1 = document.createElement('button')
  btn1.className = 'run-card-activate'
  btn1.textContent = 'open'
  card1.appendChild(btn1)

  const card2 = document.createElement('article')
  card2.setAttribute('data-run-id', '200')
  const btn2 = document.createElement('button')
  btn2.className = 'run-card-activate'
  btn2.textContent = 'open'
  card2.appendChild(btn2)

  const nonCard = document.createElement('button')
  nonCard.id = 'non-card'
  nonCard.textContent = 'not a card'

  kanban.appendChild(card1)
  kanban.appendChild(card2)
  kanban.appendChild(nonCard)
  document.body.appendChild(kanban)

  const outsideBtn = document.createElement('button')
  outsideBtn.id = 'outside'
  outsideBtn.textContent = 'outside'
  document.body.appendChild(outsideBtn)

  return {
    kanban,
    btn1,
    btn2,
    nonCard,
    outsideBtn,
  }
}

function cleanDOM() {
  document.body.innerHTML = ''
}

// ---------------------------------------------------------------------------
// focusin tests
// ---------------------------------------------------------------------------

describe('focusin', () => {
  let kanban: HTMLElement
  let btn1: HTMLElement
  let btn2: HTMLElement
  let nonCard: HTMLElement
  let ctx: ReturnType<typeof makeMockContext>

  beforeEach(() => {
    const els = buildDOM()
    kanban = els.kanban
    btn1 = els.btn1
    btn2 = els.btn2
    nonCard = els.nonCard
    ctx = makeMockContext()
  })

  afterEach(() => {
    cleanDOM()
  })

  it('sets kanbanHasFocus(true) and syncs focusedRunId when focusin hits a run-card-activate descendant', () => {
    roving(kanban, ctx)
    // Dispatch on the button itself so it bubbles up to the kanban node with
    // event.target === btn1. Constructing FocusEvent({ target }) does not set
    // the actual target in jsdom — only dispatch-on sets target.
    const ev = new FocusEvent('focusin', { bubbles: true })
    btn1.dispatchEvent(ev)

    expect(ctx._setKanbanHasFocusCalls).toEqual([true])
    expect(ctx._setFocusCalls).toEqual([100n])
  })

  it('sets kanbanHasFocus(true) but does NOT call setFocus when focusin hits a non-card-activate descendant', () => {
    roving(kanban, ctx)
    const ev = new FocusEvent('focusin', { bubbles: true })
    nonCard.dispatchEvent(ev)

    expect(ctx._setKanbanHasFocusCalls).toEqual([true])
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('parses data-run-id from the closest [data-run-id] ancestor of the .run-card-activate element', () => {
    roving(kanban, ctx)
    const ev = new FocusEvent('focusin', { bubbles: true })
    btn2.dispatchEvent(ev)

    expect(ctx._setFocusCalls).toEqual([200n])
  })

  it('does not throw and does not call setFocus when data-run-id is malformed', () => {
    // Create a button inside a card with a non-numeric data-run-id
    const badCard = document.createElement('article')
    badCard.setAttribute('data-run-id', 'not-a-bigint')
    const badBtn = document.createElement('button')
    badBtn.className = 'run-card-activate'
    badCard.appendChild(badBtn)
    kanban.appendChild(badCard)

    roving(kanban, ctx)
    expect(() => {
      const ev = new FocusEvent('focusin', { bubbles: true })
      badBtn.dispatchEvent(ev)
    }).not.toThrow()

    expect(ctx._setFocusCalls).toEqual([])
    // kanbanHasFocus IS set even on malformed id (focusin always sets it)
    expect(ctx._setKanbanHasFocusCalls).toEqual([true])
  })
})

// ---------------------------------------------------------------------------
// focusout tests
// ---------------------------------------------------------------------------

describe('focusout', () => {
  let kanban: HTMLElement
  let btn2: HTMLElement
  let outsideBtn: HTMLElement
  let ctx: ReturnType<typeof makeMockContext>

  beforeEach(() => {
    const els = buildDOM()
    kanban = els.kanban
    btn2 = els.btn2
    outsideBtn = els.outsideBtn
    ctx = makeMockContext()
    // Seed kanbanHasFocus as true (as if a prior focusin happened)
    ctx.setKanbanHasFocus(true)
    ctx._setKanbanHasFocusCalls.length = 0 // reset spy after seeding
  })

  afterEach(() => {
    cleanDOM()
  })

  it('sets kanbanHasFocus(false) when relatedTarget is outside the node', () => {
    roving(kanban, ctx)
    const ev = new FocusEvent('focusout', {
      relatedTarget: outsideBtn,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ctx._setKanbanHasFocusCalls).toEqual([false])
  })

  it('sets kanbanHasFocus(false) when relatedTarget is null', () => {
    roving(kanban, ctx)
    const ev = new FocusEvent('focusout', {
      relatedTarget: null,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ctx._setKanbanHasFocusCalls).toEqual([false])
  })

  it('does NOT call setKanbanHasFocus when focus moves to another element inside the node', () => {
    roving(kanban, ctx)
    const ev = new FocusEvent('focusout', {
      relatedTarget: btn2,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ctx._setKanbanHasFocusCalls).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// keydown tests
// ---------------------------------------------------------------------------

describe('keydown', () => {
  let kanban: HTMLElement
  let ctx: ReturnType<typeof makeMockContext>

  beforeEach(() => {
    const els = buildDOM()
    kanban = els.kanban
    ctx = makeMockContext()

    // Set up store fixtures: single card per column so ArrowDown is a no-op
    // and ArrowRight crosses to InProgress.
    // Default fixture: queued[100n], inProgress[200n], completed[300n]
    mockQueued = [createMockRun({ id: 100n, status: 'Queued' })]
    mockInProgress = [createMockRun({ id: 200n, status: 'InProgress' })]
    mockCompleted = [createMockRun({ id: 300n, status: 'Completed' })]
  })

  afterEach(() => {
    cleanDOM()
    // Reset to defaults
    mockQueued = [createMockRun({ id: 100n, status: 'Queued' })]
    mockInProgress = [createMockRun({ id: 200n, status: 'InProgress' })]
    mockCompleted = [createMockRun({ id: 300n, status: 'Completed' })]
  })

  it('calls setFocus and calls preventDefault when ArrowDown resolves to a different card (AC2.7)', () => {
    // Two-card queued column so ArrowDown has a real target
    mockQueued = [
      createMockRun({ id: 100n, status: 'Queued' }),
      createMockRun({ id: 101n, status: 'Queued' }),
    ]
    mockInProgress = []
    mockCompleted = []

    // Set current focus to first queued card
    ctx.setFocus(100n)
    ctx._setFocusCalls.length = 0

    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true, bubbles: true })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(true) // AC2.7
    expect(ctx._setFocusCalls).toEqual([101n])
  })

  it('calls preventDefault but NOT setFocus when ArrowDown is a no-op (last row of column) (AC2.7)', () => {
    // Only one card in queued — ArrowDown is a no-op
    mockQueued = [createMockRun({ id: 100n, status: 'Queued' })]
    mockInProgress = []
    mockCompleted = []

    ctx.setFocus(100n)
    ctx._setFocusCalls.length = 0

    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', { key: 'ArrowDown', cancelable: true, bubbles: true })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(true)
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('returns immediately without preventDefault when metaKey is true (AC4.1)', () => {
    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'k',
      metaKey: true,
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(false)
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('returns immediately without preventDefault when ctrlKey is true (AC4.1)', () => {
    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'k',
      ctrlKey: true,
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(false)
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('returns immediately without preventDefault when altKey is true (AC4.1)', () => {
    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'ArrowDown',
      altKey: true,
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(false)
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('returns immediately without preventDefault when shiftKey is true (AC4.1)', () => {
    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'ArrowDown',
      shiftKey: true,
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(false)
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('returns immediately without preventDefault when key is not an arrow key', () => {
    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'a',
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(false)
    expect(ctx._setFocusCalls).toEqual([])
  })

  it('calls setFocus for ArrowRight when crossing to non-empty next column', () => {
    // queued[100n], inProgress[200n] — ArrowRight from queued goes to inProgress
    ctx.setFocus(100n)
    ctx._setFocusCalls.length = 0

    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'ArrowRight',
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(true)
    expect(ctx._setFocusCalls).toEqual([200n])
  })

  it('calls preventDefault but NOT setFocus for ArrowRight when already in rightmost non-empty column', () => {
    mockQueued = []
    mockInProgress = []
    mockCompleted = [createMockRun({ id: 300n, status: 'Completed' })]

    ctx.setFocus(300n)
    ctx._setFocusCalls.length = 0

    roving(kanban, ctx)

    const ev = new KeyboardEvent('keydown', {
      key: 'ArrowRight',
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(true)
    expect(ctx._setFocusCalls).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// destroy tests
// ---------------------------------------------------------------------------

describe('destroy', () => {
  let kanban: HTMLElement
  let btn1: HTMLElement
  let ctx: ReturnType<typeof makeMockContext>

  beforeEach(() => {
    const els = buildDOM()
    kanban = els.kanban
    btn1 = els.btn1
    ctx = makeMockContext()
    mockQueued = [createMockRun({ id: 100n, status: 'Queued' })]
    mockInProgress = []
    mockCompleted = []
  })

  afterEach(() => {
    cleanDOM()
    mockQueued = [createMockRun({ id: 100n, status: 'Queued' })]
    mockInProgress = [createMockRun({ id: 200n, status: 'InProgress' })]
    mockCompleted = [createMockRun({ id: 300n, status: 'Completed' })]
  })

  it('removes all listeners after destroy — focusin and keydown produce no further calls', () => {
    const handle = roving(kanban, ctx)
    handle.destroy()

    // focusin after destroy — no calls
    const focusinEv = new FocusEvent('focusin', { bubbles: true })
    btn1.dispatchEvent(focusinEv)
    expect(ctx._setKanbanHasFocusCalls).toEqual([])
    expect(ctx._setFocusCalls).toEqual([])

    // keydown after destroy — no calls
    const keydownEv = new KeyboardEvent('keydown', {
      key: 'ArrowDown',
      cancelable: true,
      bubbles: true,
    })
    kanban.dispatchEvent(keydownEv)
    expect(ctx._setFocusCalls).toEqual([])
  })
})
