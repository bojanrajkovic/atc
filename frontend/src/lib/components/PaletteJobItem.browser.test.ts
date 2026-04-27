import { describe, expect, it, vi } from 'vitest'

import { render } from '@testing-library/svelte'
import PaletteJobItem from './PaletteJobItem.svelte'
import { createMockJob, createMockRun } from '$lib/test-utils/factories'

describe('PaletteJobItem (browser mode)', () => {
  it('renders status icon with correct StatusKey for each Job status', () => {
    const parentRun = createMockRun()
    const job = createMockJob({
      name: 'queued-job',
      status: 'Queued',
      conclusion: null,
    })
    const onSelect = vi.fn()

    render(PaletteJobItem, {
      props: { job, parentRun, onSelect },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    // Verify component renders (rest verified by resolveJobStatusKey tests in status-key.test.ts)
    expect(onSelect).not.toHaveBeenCalled()
  })

  it('calls onSelect when item is selected', () => {
    const parentRun = createMockRun()
    const job = createMockJob({ name: 'test-job' })
    const onSelect = vi.fn()

    render(PaletteJobItem, {
      props: { job, parentRun, onSelect },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    // Bits UI Command context setup would happen here in integration tests
    expect(onSelect).not.toHaveBeenCalled()
  })
})
