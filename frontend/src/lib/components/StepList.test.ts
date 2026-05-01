import { render } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import StepListWrapper from './test-utils/StepListWrapper.svelte'

describe('StepList', () => {
  const threeSteps = [
    { name: 'Set up job', statusKey: 'Success' as const, durationText: '0:03' },
    { name: 'Run tests', statusKey: 'Failure' as const, durationText: '1:12' },
    { name: 'Post cleanup', statusKey: 'Cancelled' as const, durationText: '0:01' },
  ]

  it('interactivity.AC2.1 renders multiple StepItem children in source order inside the ol', () => {
    const { container } = render(StepListWrapper, { props: { steps: threeSteps } })

    const ol = container.querySelector('ol')
    expect(ol).not.toBeNull()

    const items = ol!.querySelectorAll('li.step-item')
    expect(items).toHaveLength(3)

    expect(items[0]!.querySelector('.name')!.textContent).toBe('Set up job')
    expect(items[1]!.querySelector('.name')!.textContent).toBe('Run tests')
    expect(items[2]!.querySelector('.name')!.textContent).toBe('Post cleanup')
  })

  it('interactivity.AC2.1 container is an ol element (semantic ordered list)', () => {
    const { container } = render(StepListWrapper, {
      props: {
        steps: [{ name: 'Only step', statusKey: 'Success' as const, durationText: '0:05' }],
      },
    })

    const ol = container.querySelector('ol')
    expect(ol).not.toBeNull()
    expect(ol!.tagName.toLowerCase()).toBe('ol')
  })
})
