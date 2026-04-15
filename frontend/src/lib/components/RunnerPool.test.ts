import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import RunnerPool from './RunnerPool.svelte'

describe('RunnerPool', () => {
  it('known-capacity variant renders CapacityBar and count', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'linux',
          running: 3,
          queued: 0,
          total: 10,
          isElastic: false,
        },
      },
    })

    const meter = screen.getByRole('meter')
    expect(meter).toBeTruthy()

    const countText = screen.getByText('3/10')
    expect(countText).toBeTruthy()
  })

  it('unknown-capacity variant shows running count without CapacityBar', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'windows',
          running: 5,
          queued: 0,
          total: null,
          isElastic: false,
        },
      },
    })

    expect(screen.queryByRole('meter')).toBeNull()

    const countText = screen.getByText('5')
    expect(countText).toBeTruthy()
  })

  it('elastic variant shows running count without CapacityBar', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'macos',
          running: 2,
          queued: 0,
          total: null,
          isElastic: true,
        },
      },
    })

    expect(screen.queryByRole('meter')).toBeNull()

    const countText = screen.getByText('2')
    expect(countText).toBeTruthy()
  })

  it('shows queued badge when queued > 0 for known-capacity', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'linux',
          running: 3,
          queued: 2,
          total: 10,
          isElastic: false,
        },
      },
    })

    const badge = screen.getByText('+2 queued')
    expect(badge).toBeTruthy()
  })

  it('shows queued badge when queued > 0 for unknown-capacity', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'windows',
          running: 5,
          queued: 3,
          total: null,
          isElastic: false,
        },
      },
    })

    const badge = screen.getByText('+3 queued')
    expect(badge).toBeTruthy()
  })

  it('shows queued badge when queued > 0 for elastic', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'macos',
          running: 2,
          queued: 1,
          total: null,
          isElastic: true,
        },
      },
    })

    const badge = screen.getByText('+1 queued')
    expect(badge).toBeTruthy()
  })

  it('hides queued badge when queued is 0', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'linux',
          running: 3,
          queued: 0,
          total: 10,
          isElastic: false,
        },
      },
    })

    expect(screen.queryByText(/queued/)).toBeNull()
  })

  it('renders pool label', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'linux',
          running: 3,
          queued: 0,
          total: 10,
          isElastic: false,
        },
      },
    })

    const label = screen.getByText('linux')
    expect(label).toBeTruthy()
  })

  it('has accessible group with pool name', () => {
    render(RunnerPool, {
      props: {
        pool: {
          label: 'linux',
          running: 3,
          queued: 0,
          total: 10,
          isElastic: false,
        },
      },
    })

    const group = screen.getByRole('group', { name: /linux runner pool/i })
    expect(group).toBeTruthy()
  })
})
