import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { RunnerPoolTotal } from '$lib/types/generated/RunnerPoolTotal'
import RunnerPool from './RunnerPool.svelte'

const bounded = (value: number): RunnerPoolTotal => ({ kind: 'Bounded', value })
const unbounded: RunnerPoolTotal = { kind: 'Unbounded' }
const undeclared: RunnerPoolTotal = { kind: 'Undeclared' }

describe('RunnerPool', () => {
  it('Bounded variant renders CapacityBar and count', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
      },
    })

    const meter = screen.getByRole('meter')
    expect(meter).toBeTruthy()
    expect(screen.getByText('3/10')).toBeTruthy()
    // The unbounded affordance must NOT render on Bounded pools.
    expect(screen.queryByTestId('unbounded-affordance')).toBeNull()
  })

  it('Undeclared variant shows running count without CapacityBar or affordance', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'windows', running: 5, queued: 0, total: undeclared },
      },
    })

    expect(screen.queryByRole('meter')).toBeNull()
    expect(screen.getByText('5')).toBeTruthy()
    // No affordance — this is the WCAG 1.4.1 distinguisher from Unbounded.
    expect(screen.queryByTestId('unbounded-affordance')).toBeNull()
    expect(screen.queryByLabelText(/unbounded/i)).toBeNull()
  })

  it('Unbounded variant renders running count, no bar, and an accessible affordance', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'ubuntu-latest', running: 2, queued: 0, total: unbounded },
      },
    })

    expect(screen.queryByRole('meter')).toBeNull()
    expect(screen.getByText('2')).toBeTruthy()
    // WCAG SC 1.4.1: distinguish Unbounded from Undeclared via content,
    // not styling. The aria-label is the screen-reader-visible distinguisher.
    const affordance = screen.getByLabelText(/unbounded/i)
    expect(affordance).toBeTruthy()
    expect(screen.getByTestId('unbounded-affordance')).toBeTruthy()
  })

  it('Bounded variant shows queued badge when queued > 0', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 2, total: bounded(10) },
      },
    })

    expect(screen.getByText('+2 queued')).toBeTruthy()
  })

  it('Undeclared variant shows queued badge when queued > 0', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'windows', running: 5, queued: 3, total: undeclared },
      },
    })

    expect(screen.getByText('+3 queued')).toBeTruthy()
  })

  it('Unbounded variant shows queued badge alongside the affordance', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'macos', running: 2, queued: 1, total: unbounded },
      },
    })

    expect(screen.getByText('+1 queued')).toBeTruthy()
    expect(screen.getByLabelText(/unbounded/i)).toBeTruthy()
  })

  it('hides queued badge when queued is 0', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
      },
    })

    expect(screen.queryByText(/queued/)).toBeNull()
  })

  it('renders pool label', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
      },
    })

    expect(screen.getByText('linux')).toBeTruthy()
  })

  it('has accessible group with pool name', () => {
    render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
      },
    })

    expect(screen.getByRole('group', { name: /linux runner pool/i })).toBeTruthy()
  })

  it('omitting isActiveFilter leaves is-active-filter class absent', () => {
    const { container } = render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
      },
    })

    const root = container.querySelector('.runner-pool')
    expect(root).not.toBeNull()
    expect(root!.classList.contains('is-active-filter')).toBe(false)
  })

  it('isActiveFilter={false} leaves is-active-filter class absent', () => {
    const { container } = render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
        isActiveFilter: false,
      },
    })

    const root = container.querySelector('.runner-pool')
    expect(root).not.toBeNull()
    expect(root!.classList.contains('is-active-filter')).toBe(false)
  })

  it('isActiveFilter={true} adds is-active-filter class to root', () => {
    const { container } = render(RunnerPool, {
      props: {
        pool: { label: 'linux', running: 3, queued: 0, total: bounded(10) },
        isActiveFilter: true,
      },
    })

    const root = container.querySelector('.runner-pool')
    expect(root).not.toBeNull()
    expect(root!.classList.contains('is-active-filter')).toBe(true)
  })
})
