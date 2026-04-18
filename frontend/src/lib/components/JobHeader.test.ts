import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import JobHeader from './JobHeader.svelte'

describe('JobHeader', () => {
  it('renders displayTitle as visible text', () => {
    render(JobHeader, {
      props: { displayTitle: 'Deploy to production', statusValue: 'Success', durationText: '2:14' },
    })
    expect(screen.getByText('Deploy to production')).toBeTruthy()
  })

  it('renders durationText as visible text', () => {
    render(JobHeader, {
      props: { displayTitle: 'CI', statusValue: 'InProgress', durationText: '1:30' },
    })
    expect(screen.getByText('1:30')).toBeTruthy()
  })

  it('composes a StatusIcon — glyph for the given statusValue appears', () => {
    render(JobHeader, {
      props: { displayTitle: 'CI', statusValue: 'Success', durationText: '2:14' },
    })
    // StatusIcon for Success renders ✓ glyph
    expect(screen.getByText('✓')).toBeTruthy()
  })

  it('boundary: empty durationText — duration span present but empty', () => {
    const { container } = render(JobHeader, {
      props: { displayTitle: 'CI', statusValue: 'Queued', durationText: '' },
    })
    const duration = container.querySelector('.run-card-duration')
    expect(duration).toBeTruthy()
    expect(duration?.textContent?.trim()).toBe('')
  })

  it('boundary: long displayTitle — run-card-name element has truncate class', () => {
    const longTitle = 'A'.repeat(200)
    const { container } = render(JobHeader, {
      props: { displayTitle: longTitle, statusValue: 'Queued', durationText: '0:00' },
    })
    const name = container.querySelector('.run-card-name')
    expect(name).toBeTruthy()
    expect(name?.className).toContain('truncate')
  })
})
