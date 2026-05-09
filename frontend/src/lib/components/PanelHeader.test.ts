import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { StatusKey } from '$lib/format/status-key'

import PanelHeader from './PanelHeader.svelte'

describe('PanelHeader', () => {
  // Parameterized over all 11 StatusKey variants.
  // Expected token names are hand-written (not derived from statusKeyToVar)
  // so the test is a real behavioral assertion, not a tautology.
  it.each([
    ['Queued', 'Queued', 'var(--queued)'],
    ['InProgress', 'In progress', 'var(--running)'],
    ['Success', 'Success', 'var(--success)'],
    ['Failure', 'Failure', 'var(--failed)'],
    ['Cancelled', 'Cancelled', 'var(--cancelled)'],
    ['TimedOut', 'Timed out', 'var(--timed-out)'],
    ['ActionRequired', 'Action required', 'var(--action-required)'],
    ['StartupFailure', 'Startup failure', 'var(--failed)'],
    ['Stale', 'Stale', 'var(--neutral)'],
    ['Neutral', 'Neutral', 'var(--neutral)'],
    ['Skipped', 'Skipped', 'var(--neutral)'],
  ] as const satisfies ReadonlyArray<
    [StatusKey, string, string]
  >)('renders statusKey=%s with correct data-status-key, eyebrow label, and --status-color token', (statusKey, statusLabel, expectedVar) => {
    render(PanelHeader, {
      props: { statusKey, statusLabel, title: 'My workflow run' },
    })

    // Assert data-status-key attribute on the header element
    const header = document.querySelector('[data-status-key]')
    expect(header).not.toBeNull()
    expect(header!.getAttribute('data-status-key')).toBe(statusKey)

    // Assert the inline --status-color style contains the expected var(--token)
    const style = (header as HTMLElement).getAttribute('style')
    expect(style).toContain(`--status-color: ${expectedVar}`)

    // Assert the eyebrow statusLabel text renders (glyph/color shown)
    expect(screen.getByText(statusLabel)).toBeTruthy()
  })

  it('renders the run title as visible heading text', () => {
    render(PanelHeader, {
      props: { statusKey: 'Success', statusLabel: 'Success', title: 'Deploy to production' },
    })

    expect(screen.getByText('Deploy to production')).toBeTruthy()
  })
})
