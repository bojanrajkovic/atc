import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterAll, afterEach, beforeEach, describe, expect, it } from 'vitest'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRunEvent } from '$lib/test-utils/factories'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'

// RunDetailPanel is rendered via its test harness so that getRovingContext()
// (added in kanban-keyboard-nav Phase 4) resolves correctly inside jsdom tests.
// The harness wraps RunDetailPanel in a real RovingFocusProvider with an empty
// synthetic-kanban div — all panel behavior under test is unchanged.
import RunDetailPanel from './RunDetailPanel.test-harness.svelte'

// Shared fixture run id used by most tests.
const RUN_ID = 42n

// Seed a run in runStore using the given id and optional overrides.
function seedRun(runId: bigint, overrides: Partial<Parameters<typeof createMockRunEvent>[0]> = {}) {
  runStore.applyRunEvent(
    createMockRunEvent({
      runId,
      action: { type: 'InProgress' },
      displayTitle: 'CI — main',
      htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/42',
      headSha: 'abc1234567890',
      runStartedAt: '2026-04-27T10:00:00Z',
      ...overrides,
    }),
  )
}

describe('RunDetailPanel', () => {
  beforeEach(() => {
    // Clean up any rendered components from the prior test.
    cleanup()
    // Reset stores to a clean state before each test.
    runStore.clear()
    uiStore.selectedRunId = null
    uiStore.selectedJobId = null
  })

  // RunDetailPanel transitively imports uiStore which starts a 1s setInterval.
  // Destroy it once when the file is torn down to prevent timer leaks.
  afterAll(() => {
    uiStore.destroy()
  })

  // -----------------------------------------------------------------------
  // AC2.1 — Panel opens when selectedRunId is set to a valid run
  // -----------------------------------------------------------------------
  it('interactivity.AC2.1 panel renders single-pane layout when selectedRunId set to existing run', async () => {
    seedRun(RUN_ID)
    render(RunDetailPanel)

    uiStore.selectedRunId = RUN_ID
    await tick()

    // Bits UI Dialog portal mounts to document.body — must use screen.*, not container.*
    const dialog = await waitFor(() => screen.getByRole('dialog'))
    expect(dialog).toBeTruthy()

    // Panel header section present
    expect(document.querySelector('.panel-header')).toBeTruthy()

    // Meta grid section present
    expect(document.querySelector('.meta-grid')).toBeTruthy()
  })

  it('interactivity.AC2.1 panel does NOT render when selectedRunId is null', () => {
    render(RunDetailPanel)
    // selectedRunId is null — {#if run} block should not mount the Sheet
    expect(document.querySelector('.panel-header')).toBeNull()
    expect(document.querySelector('.meta-grid')).toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC2.2 — "Go to run" link attributes
  // -----------------------------------------------------------------------
  it('interactivity.AC2.2 Go-to-run anchor has correct href, target, and rel', async () => {
    const htmlUrl = 'https://github.com/test-org/test-repo/actions/runs/42'
    seedRun(RUN_ID, { htmlUrl })
    render(RunDetailPanel)

    uiStore.selectedRunId = RUN_ID
    await tick()

    await waitFor(() => screen.getByRole('link', { name: /go to run/i }))
    const link = screen.getByRole('link', { name: /go to run/i })

    expect(link.getAttribute('href')).toBe(htmlUrl)
    expect(link.getAttribute('target')).toBe('_blank')
    expect(link.getAttribute('rel')).toBe('noopener noreferrer')
  })

  // -----------------------------------------------------------------------
  // AC2.3 — Esc key closes panel and clears selectedRunId
  // -----------------------------------------------------------------------
  it('interactivity.AC2.3 Esc key clears selectedRunId and unmounts the sheet', async () => {
    seedRun(RUN_ID)
    render(RunDetailPanel)

    uiStore.selectedRunId = RUN_ID
    await tick()

    await waitFor(() => screen.getByRole('dialog'))
    expect(uiStore.selectedRunId).toBe(RUN_ID)

    // Bits UI Dialog listens at document level for Escape.
    fireEvent.keyDown(document.body, { key: 'Escape', code: 'Escape' })
    await tick()

    // selectedRunId should be cleared
    await waitFor(() => {
      expect(uiStore.selectedRunId).toBeNull()
    })

    // Sheet content should be unmounted (the {#if run} block evaluates to false once
    // selectedRunId is null and runStore.runs no longer returns the run).
    await waitFor(() => {
      expect(document.querySelector('.panel-header')).toBeNull()
    })
  })

  // -----------------------------------------------------------------------
  // AC2.4 — Close button ("Close detail panel") closes panel
  // -----------------------------------------------------------------------
  it('interactivity.AC2.4 clicking "Close detail panel" button clears selectedRunId', async () => {
    seedRun(RUN_ID)
    render(RunDetailPanel)

    uiStore.selectedRunId = RUN_ID
    await tick()

    await waitFor(() => screen.getByRole('button', { name: 'Close detail panel' }))
    const closeBtn = screen.getByRole('button', { name: 'Close detail panel' })

    await fireEvent.click(closeBtn)
    await tick()

    await waitFor(() => {
      expect(uiStore.selectedRunId).toBeNull()
    })
  })

  // -----------------------------------------------------------------------
  // AC2.8 — All 11 StatusKey fixtures render with correct data-status-key
  // -----------------------------------------------------------------------

  // Mapping from StatusKey → (status, conclusion) that resolves to it.
  const STATUS_KEY_FIXTURES = [
    { key: 'Queued', status: 'Queued', conclusion: null },
    { key: 'InProgress', status: 'InProgress', conclusion: null },
    { key: 'Success', status: 'Completed', conclusion: 'Success' },
    { key: 'Failure', status: 'Completed', conclusion: 'Failure' },
    { key: 'Cancelled', status: 'Completed', conclusion: 'Cancelled' },
    { key: 'TimedOut', status: 'Completed', conclusion: 'TimedOut' },
    { key: 'ActionRequired', status: 'Completed', conclusion: 'ActionRequired' },
    { key: 'StartupFailure', status: 'Completed', conclusion: 'StartupFailure' },
    { key: 'Stale', status: 'Completed', conclusion: 'Stale' },
    { key: 'Neutral', status: 'Completed', conclusion: 'Neutral' },
    { key: 'Skipped', status: 'Completed', conclusion: 'Skipped' },
  ] as const

  it.each(
    STATUS_KEY_FIXTURES,
  )('interactivity.AC2.8 PanelHeader has data-status-key="$key" for $key run', async ({
    key,
    status,
    conclusion,
  }) => {
    const runId = RUN_ID + 1000n

    // Use createMockRunEvent overrides directly to set status+conclusion.
    // createMockRunEvent maps to action types, so we use applyRunEvent then
    // directly set the conclusion on the stored WorkflowRun via re-applying
    // a Completed event with the right conclusion.
    if (status === 'Queued') {
      runStore.applyRunEvent(createMockRunEvent({ runId, action: { type: 'Requested' } }))
    } else if (status === 'InProgress') {
      runStore.applyRunEvent(
        createMockRunEvent({
          runId,
          action: { type: 'InProgress' },
          runStartedAt: '2026-04-27T10:00:00Z',
        }),
      )
    } else {
      // Completed with a specific conclusion — conclusion is a narrowed
      // string literal from the as-const fixture; cast to RunConclusion.
      runStore.applyRunEvent(
        createMockRunEvent({
          runId,
          action: {
            type: 'Completed',
            data: { conclusion: conclusion as RunConclusion },
          },
        }),
      )
    }

    render(RunDetailPanel)
    uiStore.selectedRunId = runId
    await tick()

    const header = await waitFor(() => document.querySelector('.panel-header'))
    expect(header).toBeTruthy()
    expect(header!.getAttribute('data-status-key')).toBe(key)

    // Teardown: close panel before next iteration
    cleanup()
    uiStore.selectedRunId = null
    runStore.clear()
  })

  // -----------------------------------------------------------------------
  // AC2.9 — Missing-run fallback: selectedRunId for non-existent run is cleared
  // -----------------------------------------------------------------------
  it('interactivity.AC2.9 selectedRunId is cleared when referencing a run not in runStore', async () => {
    render(RunDetailPanel)

    // 99999n is not seeded in runStore — fallback $effect should clear it.
    uiStore.selectedRunId = 99999n
    await tick()

    await waitFor(() => {
      expect(uiStore.selectedRunId).toBeNull()
    })

    // No dialog should be open
    expect(document.querySelector('.panel-header')).toBeNull()
  })

  it('interactivity.AC2.9 selectedRunId stays null when null (no loop)', async () => {
    render(RunDetailPanel)
    // Starting at null — the fallback $effect should not trigger or loop.
    uiStore.selectedRunId = null
    await tick()
    expect(uiStore.selectedRunId).toBeNull()
  })

  // -----------------------------------------------------------------------
  // Additional: after close via selectedRunId = null, panel unmounts cleanly
  // -----------------------------------------------------------------------
  it('interactivity.AC2.1 panel unmounts when selectedRunId set back to null', async () => {
    seedRun(RUN_ID)
    render(RunDetailPanel)

    uiStore.selectedRunId = RUN_ID
    await tick()
    await waitFor(() => document.querySelector('.panel-header'))

    // Clear selectedRunId — panel should unmount
    uiStore.selectedRunId = null
    await tick()

    await waitFor(() => {
      expect(document.querySelector('.panel-header')).toBeNull()
    })
  })
})

// afterEach cleanup ensures each test's rendered tree is properly torn down.
// This prevents portal-content from one test leaking into the next.
afterEach(() => {
  cleanup()
})
