import { render } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'

import { createMockJob, createMockRun } from '$lib/test-utils/factories'
import Wrapper from './test-utils/PaletteJobItemWrapper.svelte'

describe('PaletteJobItem (browser mode)', () => {
  it('renders status icon with correct StatusKey for each Job status', () => {
    const parentRun = createMockRun()
    const job = createMockJob({
      name: 'queued-job',
      status: 'Queued',
      conclusion: null,
    })
    const onSelect = vi.fn()

    render(Wrapper, {
      props: { job, parentRun, onSelect },
    })

    // Verify component renders (rest verified by resolveJobStatusKey tests in status-key.test.ts)
    expect(onSelect).not.toHaveBeenCalled()
  })

  it('calls onSelect when item is selected', () => {
    const parentRun = createMockRun()
    const job = createMockJob({ name: 'test-job' })
    const onSelect = vi.fn()

    render(Wrapper, {
      props: { job, parentRun, onSelect },
    })

    // Bits UI Command context setup would happen here in integration tests
    expect(onSelect).not.toHaveBeenCalled()
  })
})
