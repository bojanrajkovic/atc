import { render } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import JobMeta from './JobMeta.svelte'

describe('JobMeta', () => {
  describe('run-cards.AC7.1: renders repo and branch with middle-dot separator', () => {
    it('renders repo and branch with visible separator', () => {
      const { container } = render(JobMeta, {
        props: { repo: 'my-org/api', branch: 'main' },
      })
      expect(container.textContent?.includes('my-org/api')).toBe(true)
      expect(container.textContent?.includes('main')).toBe(true)
      // Verify the middle-dot separator is rendered
      expect(container.textContent?.includes('·')).toBe(true)
    })
  })

  describe('run-cards.AC7.2: null branch renders repo only without separator', () => {
    it('renders only repo when branch is null', () => {
      const { container } = render(JobMeta, {
        props: { repo: 'my-org/api', branch: null },
      })
      expect(container.textContent?.trim()).toBe('my-org/api')
      // Ensure no separator is rendered
      expect(container.textContent?.includes('·')).toBe(false)
    })

    it('renders only repo when branch is empty string', () => {
      const { container } = render(JobMeta, {
        props: { repo: 'my-org/api', branch: '' },
      })
      expect(container.textContent?.trim()).toBe('my-org/api')
      // Ensure no separator is rendered
      expect(container.textContent?.includes('·')).toBe(false)
    })
  })

  describe('run-cards.AC7.3: long text uses text-overflow: ellipsis + white-space: nowrap + overflow: hidden', () => {
    it('includes truncate class for overflow handling', () => {
      const { container } = render(JobMeta, {
        props: { repo: 'my-org/api', branch: 'main' },
      })
      const element = container.querySelector('.run-card-meta')
      expect(element?.className.includes('truncate')).toBe(true)
    })
  })

  describe('additional: run-card-meta class is present', () => {
    it('has run-card-meta class on root element', () => {
      const { container } = render(JobMeta, {
        props: { repo: 'my-org/api', branch: 'main' },
      })
      const element = container.querySelector('.run-card-meta')
      expect(element).toBeTruthy()
    })
  })

  describe('additional: middle-dot is aria-hidden', () => {
    it('wraps separator in aria-hidden span', () => {
      const { container } = render(JobMeta, {
        props: { repo: 'my-org/api', branch: 'main' },
      })
      const hiddenSpan = container.querySelector('[aria-hidden="true"]')
      expect(hiddenSpan?.textContent).toBe('·')
    })
  })
})
