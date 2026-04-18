import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as durationTextModule from '$lib/format/duration-text'
import type { JobStats } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRun } from '$lib/test-utils/factories'

import RunCard from './RunCard.svelte'

/**
 * AC12.7 — the static-Completed branch of RunCard's durationText $derived
 * MUST NOT subscribe to uiStore.nowMs. If it did, a future refactor that
 * accidentally hoists the nowMs read above the branch split would be silent:
 * text remains '2:14' because static inputs are constant, but the derivation
 * re-evaluates on every tick and call count grows.
 *
 * Technique: static imports (avoids vi.resetModules breaking Svelte runtime),
 * direct uiStore.nowMs assignment (avoids fake-timer interaction with the
 * module-scope singleton), and a spy on the pure computeDurationText module
 * to count reactive re-evaluations.
 */
describe('RunCard — AC12.7 static-Completed derivation does not re-evaluate on tick', () => {
  const emptyJobStats: JobStats = { completed: 0, total: 0, runnerSummary: null }
  const T0 = new Date('2026-04-17T10:00:00Z').getTime()
  let savedNowMs: number

  beforeEach(() => {
    savedNowMs = uiStore.nowMs
    uiStore.nowMs = T0
  })

  afterEach(() => {
    uiStore.nowMs = savedNowMs
    vi.restoreAllMocks()
  })

  // Static import of RunCard transitively imports uiStore, whose constructor
  // starts a 1s setInterval. File-scope afterAll stops it so the timer does
  // not outlive this test file and leak into subsequent ones.
  afterAll(() => {
    uiStore.destroy()
  })

  it('static-Completed card: computeDurationText not called when nowMs changes', async () => {
    const computeSpy = vi.spyOn(durationTextModule, 'computeDurationText')

    const run = createMockRun({
      status: 'Completed',
      conclusion: 'Success',
      runStartedAt: '2026-04-17T09:00:00Z',
      updatedAt: '2026-04-17T09:02:14Z',
    })

    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    await tick()

    const durationEl = container.querySelector('.run-card-duration')
    expect(durationEl?.textContent?.trim()).toBe('2:14')

    const callsBefore = computeSpy.mock.calls.length

    // Simulate 10s of wall-clock passing.
    uiStore.nowMs = T0 + 10_000
    await tick()

    // Text unchanged — static inputs produce same output.
    expect(durationEl?.textContent?.trim()).toBe('2:14')
    // DOM node not replaced — same element reference.
    expect(container.querySelector('.run-card-duration')).toBe(durationEl)
    // The derivation did NOT re-evaluate. If the static branch accidentally
    // read uiStore.nowMs, the $derived would re-run and computeDurationText
    // would be called again — this assertion catches that regression.
    expect(computeSpy.mock.calls.length).toBe(callsBefore)
  })

  it('sanity: live (InProgress) card DOES re-evaluate on nowMs change', async () => {
    const computeSpy = vi.spyOn(durationTextModule, 'computeDurationText')

    const run = createMockRun({
      status: 'InProgress',
      runStartedAt: '2026-04-17T09:58:00Z',
    })

    const { container } = render(RunCard, { props: { run, jobStats: emptyJobStats } })
    await tick()

    const durationEl = container.querySelector('.run-card-duration')
    expect(durationEl?.textContent?.trim()).toBe('2:00')

    const callsBefore = computeSpy.mock.calls.length

    uiStore.nowMs = T0 + 1000
    await tick()

    // Text updates.
    expect(durationEl?.textContent?.trim()).toBe('2:01')
    // And computeDurationText WAS re-invoked.
    expect(computeSpy.mock.calls.length).toBeGreaterThan(callsBefore)
  })
})
