import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { StatusKey } from '$lib/format/status-key'

import StepItem from './StepItem.svelte'

describe('StepItem', () => {
  // --- interactivity.AC2.8 ---
  // Parameterized over all 11 StatusKey variants.
  // Expected token names are hand-written (not derived from statusKeyToVar)
  // so the test is a real behavioral assertion, not a tautology.
  it.each([
    ['Queued', 'var(--queued)'],
    ['InProgress', 'var(--running)'],
    ['Success', 'var(--success)'],
    ['Failure', 'var(--failed)'],
    ['Cancelled', 'var(--cancelled)'],
    ['TimedOut', 'var(--timed-out)'],
    ['ActionRequired', 'var(--action-required)'],
    ['StartupFailure', 'var(--failed)'],
    ['Stale', 'var(--neutral)'],
    ['Neutral', 'var(--neutral)'],
    ['Skipped', 'var(--neutral)'],
  ] as const satisfies ReadonlyArray<
    [StatusKey, string]
  >)('interactivity.AC2.8 renders statusKey=%s with correct data-status-key, --status-color, name, and duration', (statusKey, expectedVar) => {
    render(StepItem, {
      props: {
        statusKey,
        name: 'Set up job',
        durationText: '0:42',
      },
    })

    // 1. data-status-key attribute matches
    const item = document.querySelector('[data-status-key]')
    expect(item).not.toBeNull()
    expect(item!.getAttribute('data-status-key')).toBe(statusKey)

    // 2. --status-color inline style matches the expected var(--token)
    const style = (item as HTMLElement).getAttribute('style')
    expect(style).toContain(`--status-color: ${expectedVar}`)

    // 3. Name text renders
    expect(screen.getByText('Set up job')).toBeTruthy()

    // 4. Duration text renders
    expect(screen.getByText('0:42')).toBeTruthy()
  })
})
