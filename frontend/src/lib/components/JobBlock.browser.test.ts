import { render } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createMockJob } from '$lib/test-utils/factories'
import JobBlock from './JobBlock.svelte'

describe('JobBlock (browser mode)', () => {
  beforeEach(() => {
    vi.spyOn(Element.prototype, 'scrollIntoView').mockImplementation(() => {})
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('calls scrollIntoView({ block: start, behavior: smooth }) under RAF when selectedJobId matches', async () => {
    const job = createMockJob({ id: 99n, status: 'InProgress' })
    const consumeCallback = vi.fn()

    const { rerender } = render(JobBlock, {
      props: {
        job,
        durationText: '1:23',
        selectedJobId: null,
        onSelectedJobIdConsumed: consumeCallback,
      },
    })

    // Trigger the $effect by setting selectedJobId to match job.id
    await rerender({
      job,
      durationText: '1:23',
      selectedJobId: job.id,
      onSelectedJobIdConsumed: consumeCallback,
    })

    // Wait one animation frame for the RAF callback to fire
    await new Promise<void>((r) => requestAnimationFrame(() => r()))

    const scrollSpy = vi.mocked(Element.prototype.scrollIntoView)

    // scrollIntoView must have been called with the correct args
    expect(scrollSpy).toHaveBeenCalledWith({ block: 'start', behavior: 'smooth' })

    // onSelectedJobIdConsumed must have been invoked exactly once
    expect(consumeCallback).toHaveBeenCalledTimes(1)
  })
})
