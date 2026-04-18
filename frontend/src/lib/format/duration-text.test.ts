import { describe, expect, it } from 'vitest'
import { createMockRun } from '$lib/test-utils/factories'
import { computeDurationText } from './duration-text'

const T0 = new Date('2026-04-17T10:00:00Z').getTime()

describe('computeDurationText', () => {
  it('AC12.1: Queued run returns "waiting MM:SS" relative to nowMs', () => {
    const run = createMockRun({
      status: 'Queued',
      createdAt: '2026-04-17T09:59:00Z',
    })
    expect(computeDurationText(run, T0)).toBe('waiting 1:00')
    expect(computeDurationText(run, T0 + 1000)).toBe('waiting 1:01')
  })

  it('AC12.2: InProgress run returns elapsed MM:SS relative to nowMs', () => {
    const run = createMockRun({
      status: 'InProgress',
      runStartedAt: '2026-04-17T09:58:00Z',
    })
    expect(computeDurationText(run, T0)).toBe('2:00')
    expect(computeDurationText(run, T0 + 1000)).toBe('2:01')
  })

  it('AC12.3: Completed+ActionRequired returns "awaiting action MM:SS" relative to nowMs', () => {
    const run = createMockRun({
      status: 'Completed',
      conclusion: 'ActionRequired',
      updatedAt: '2026-04-17T09:59:30Z',
    })
    expect(computeDurationText(run, T0)).toBe('awaiting action 0:30')
    expect(computeDurationText(run, T0 + 1000)).toBe('awaiting action 0:31')
  })

  it('AC12.4: Completed+Success returns static MM:SS that ignores nowMs', () => {
    const run = createMockRun({
      status: 'Completed',
      conclusion: 'Success',
      runStartedAt: '2026-04-17T09:00:00Z',
      updatedAt: '2026-04-17T09:02:14Z',
    })
    // Identical across very different nowMs values — proves nowMs is not read in static branch.
    expect(computeDurationText(run, T0)).toBe('2:14')
    expect(computeDurationText(run, T0 + 10_000)).toBe('2:14')
    expect(computeDurationText(run, 0)).toBe('2:14')
  })

  it('AC12.5: InProgress with null runStartedAt falls back to createdAt', () => {
    const run = createMockRun({
      status: 'InProgress',
      runStartedAt: null,
      createdAt: '2026-04-17T09:59:00Z',
    })
    expect(computeDurationText(run, T0)).toBe('1:00')
  })

  it('AC12.6: Completed+Success with null runStartedAt returns em dash', () => {
    const run = createMockRun({
      status: 'Completed',
      conclusion: 'Success',
      runStartedAt: null,
      updatedAt: '2026-04-17T09:02:14Z',
    })
    expect(computeDurationText(run, T0)).toBe('\u2014')
    // Stable across nowMs changes.
    expect(computeDurationText(run, T0 + 10_000)).toBe('\u2014')
  })

  it('Completed+Failure (any non-ActionRequired conclusion) also takes the static branch', () => {
    const run = createMockRun({
      status: 'Completed',
      conclusion: 'Failure',
      runStartedAt: '2026-04-17T09:00:00Z',
      updatedAt: '2026-04-17T09:02:14Z',
    })
    expect(computeDurationText(run, T0)).toBe('2:14')
    expect(computeDurationText(run, T0 + 10_000)).toBe('2:14')
  })

  it('Long durations (> 1 hour) render as H:MM:SS', () => {
    const run = createMockRun({
      status: 'Completed',
      conclusion: 'Success',
      runStartedAt: '2026-04-17T08:00:00Z',
      updatedAt: '2026-04-17T09:30:45Z',
    })
    expect(computeDurationText(run, T0)).toBe('1:30:45')
  })
})
