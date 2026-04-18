import { render, screen } from '@testing-library/svelte'
import { afterAll, describe, expect, it } from 'vitest'
import type { JobStats } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRun } from '$lib/test-utils/factories'

import type { RunCardProps } from './RunCard.svelte'
import RunCard from './RunCard.svelte'

const emptyJobStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

describe('RunCard', () => {
  // Static import of RunCard transitively imports uiStore, whose constructor
  // starts a 1s setInterval. File-scope afterAll stops it so the timer does
  // not outlive this test file and leak into subsequent ones.
  afterAll(() => {
    uiStore.destroy()
  })
  it('renders displayTitle as visible text (AC4.1)', () => {
    const run = createMockRun({ displayTitle: 'Test Workflow Run' })
    render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    const title = screen.getByText('Test Workflow Run')
    expect(title).toBeTruthy()
  })

  it('renders status indicator with Queued status (AC4.2)', () => {
    const run = createMockRun({ status: 'Queued' })
    render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    // StatusIcon for Queued renders ◐ glyph
    const glyph = screen.getByText('\u25D0')
    expect(glyph).toBeTruthy()

    const srOnly = screen.getByText('Queued')
    expect(srOnly).toBeTruthy()
    expect(srOnly.className).toContain('sr-only')
  })

  it('renders status indicator with InProgress status (AC4.2)', () => {
    const run = createMockRun({ status: 'InProgress' })
    render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    // StatusIcon for InProgress renders ▶ glyph
    const glyph = screen.getByText('\u25B6')
    expect(glyph).toBeTruthy()

    const srOnly = screen.getByText('In Progress')
    expect(srOnly).toBeTruthy()
    expect(srOnly.className).toContain('sr-only')
  })

  it('renders status indicator with Completed status (AC4.2)', () => {
    // Completed with no conclusion → resolveStatusKey returns 'Cancelled'
    const run = createMockRun({ status: 'Completed' })
    render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    // StatusIcon for Cancelled renders ⊘ glyph
    const glyph = screen.getByText('\u2298')
    expect(glyph).toBeTruthy()

    // sr-only label is 'Cancelled'
    const srOnly = screen.getByText('Cancelled')
    expect(srOnly).toBeTruthy()
    expect(srOnly.className).toContain('sr-only')
  })

  it('applies correct color variable for Queued status (AC4.2)', () => {
    const run = createMockRun({ status: 'Queued' })
    const { container } = render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    const root = container.querySelector('.run-card')
    expect(root?.getAttribute('style')).toContain('--status-color: var(--queued)')
  })

  it('applies correct color variable for InProgress status (AC4.2)', () => {
    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    const root = container.querySelector('.run-card')
    expect(root?.getAttribute('style')).toContain('--status-color: var(--running)')
  })

  it('applies correct color variable for Completed status (AC4.2)', () => {
    // Completed with no conclusion → Cancelled → var(--cancelled)
    const run = createMockRun({ status: 'Completed' })
    const { container } = render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    const root = container.querySelector('.run-card')
    expect(root?.getAttribute('style')).toContain('--status-color: var(--cancelled)')
  })

  it('has data-run-id attribute for test targeting', () => {
    const run = createMockRun({ id: 456n })
    const { container } = render(RunCard, {
      props: { run, jobStats: emptyJobStats },
    })

    const element = container.querySelector('[data-run-id]')
    expect(element).toBeTruthy()
    expect(element?.getAttribute('data-run-id')).toBe('456')
  })

  describe('RunCard — Sub-Phase 4 composition', () => {
    // AC10.1: scope-contract comment removed — reviewer-verified (see Task 3 commit).

    it('AC10.2: sets --status-color to the correct CSS variable for each of the 11 StatusKeys', () => {
      // Every branch in resolveStatusColorVar must be exercised here —
      // missing one means Codecov flags uncovered code AND a future refactor
      // could silently break a single StatusKey without any test failing.
      const cases: Array<[Parameters<typeof createMockRun>[0], string]> = [
        [{ status: 'Queued' }, 'var(--queued)'],
        [{ status: 'InProgress' }, 'var(--running)'],
        [{ status: 'Completed', conclusion: 'Success' }, 'var(--success)'],
        [{ status: 'Completed', conclusion: 'Failure' }, 'var(--failed)'],
        [{ status: 'Completed', conclusion: 'Cancelled' }, 'var(--cancelled)'],
        [{ status: 'Completed', conclusion: 'TimedOut' }, 'var(--timed-out)'],
        [{ status: 'Completed', conclusion: 'ActionRequired' }, 'var(--action-required)'],
        [{ status: 'Completed', conclusion: 'StartupFailure' }, 'var(--failed)'],
        [{ status: 'Completed', conclusion: 'Stale' }, 'var(--neutral)'],
        [{ status: 'Completed', conclusion: 'Neutral' }, 'var(--neutral)'],
        [{ status: 'Completed', conclusion: 'Skipped' }, 'var(--neutral)'],
        // bare Completed (conclusion: null) resolves to Cancelled per AC6A.4.
        [{ status: 'Completed', conclusion: null }, 'var(--cancelled)'],
      ]

      for (const [runOverride, expectedColor] of cases) {
        const { container, unmount } = render(RunCard, {
          props: { run: createMockRun(runOverride), jobStats: emptyJobStats },
        })
        const root = container.querySelector('.run-card')
        expect(root?.getAttribute('style')).toContain(`--status-color: ${expectedColor}`)
        unmount()
      }
    })

    it('AC10.3: sets data-status to the exact PascalCase RunStatus value', () => {
      const cases = [
        { status: 'Queued' as const },
        { status: 'InProgress' as const },
        { status: 'Completed' as const },
      ]

      for (const override of cases) {
        const { container, unmount } = render(RunCard, {
          props: { run: createMockRun(override), jobStats: emptyJobStats },
        })
        const root = container.querySelector('.run-card')
        expect(root?.getAttribute('data-status')).toBe(override.status)
        unmount()
      }
    })

    it('AC10.5: composes all five leaf components', () => {
      const run = createMockRun({ status: 'Queued', repo: 'acme', branch: 'main' })
      const { container } = render(RunCard, {
        props: {
          run,
          jobStats: { completed: 1, total: 3, runnerSummary: 'ubuntu-latest' },
        },
      })

      // StatusIcon glyph (◐ for Queued)
      expect(screen.getByText('\u25D0')).toBeTruthy()
      // JobHeader wrapper class
      expect(container.querySelector('.run-card-header')).toBeTruthy()
      // JobMeta wrapper class
      expect(container.querySelector('.run-card-meta')).toBeTruthy()
      // ProgressBar via its label text
      expect(screen.getByText('Jobs 1 of 3')).toBeTruthy()
      // RunnerLabel ⊞ glyph (U+229E, present when summary is non-null)
      expect(screen.getByText('\u229E', { exact: false })).toBeTruthy()
    })

    it('AC10.6: RunCardProps exported interface has correct shape', () => {
      // Type-level assertion: fails compilation if RunCardProps is missing or wrong shape
      const _sample: RunCardProps = {
        run: createMockRun(),
        jobStats: { completed: 0, total: 0, runnerSummary: null },
      }
      expect(_sample).toBeTruthy()
    })

    // AC10.7 reviewer guidance: only non-prop reactive read in RunCard.svelte is uiStore.nowMs.
    // Behavioural proof lives in RunCard.duration.test.ts AC12.7.
  })
})
