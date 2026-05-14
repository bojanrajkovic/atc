import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import CapacityBar from './CapacityBar.svelte'

describe('CapacityBar', () => {
  it('renders green fill when utilization below 70%', () => {
    const { container } = render(CapacityBar, { props: { used: 2, total: 10 } })
    const meter = screen.getByRole('meter')
    expect(meter).toBeTruthy()
    const fillBar = container.querySelector('[style*="width"]')
    expect(fillBar?.getAttribute('style')).toContain('var(--success)')
  })

  it('renders amber fill when utilization between 70% and 99%', () => {
    const { container } = render(CapacityBar, { props: { used: 7, total: 10 } })
    const meter = screen.getByRole('meter')
    expect(meter).toBeTruthy()
    const fillBar = container.querySelector('[style*="width"]')
    expect(fillBar?.getAttribute('style')).toContain('var(--running)')
  })

  it('renders red fill when utilization is 100%', () => {
    const { container } = render(CapacityBar, { props: { used: 5, total: 5 } })
    const meter = screen.getByRole('meter')
    expect(meter).toBeTruthy()
    const fillBar = container.querySelector('[style*="width"]')
    expect(fillBar?.getAttribute('style')).toContain('var(--failed)')
  })

  it('sets correct ARIA attributes', () => {
    render(CapacityBar, { props: { used: 3, total: 10 } })
    const meter = screen.getByRole('meter')
    expect(meter.getAttribute('aria-valuenow')).toBe('3')
    expect(meter.getAttribute('aria-valuemin')).toBe('0')
    expect(meter.getAttribute('aria-valuemax')).toBe('10')
  })

  it('renders correct fill width percentage', () => {
    const { container } = render(CapacityBar, { props: { used: 5, total: 10 } })
    const fillBar = container.querySelector('[style*="width"]')
    expect(fillBar?.getAttribute('style')).toContain('width: 50%')
  })

  it('handles zero total gracefully', () => {
    const { container } = render(CapacityBar, { props: { used: 0, total: 0 } })
    const meter = screen.getByRole('meter')
    expect(meter).toBeTruthy()
    const fillBar = container.querySelector('[style*="width"]')
    expect(fillBar?.getAttribute('style')).toContain('width: 0%')
  })

  it('clamps fill width at 100% and stays --failed when over capacity', () => {
    const { container } = render(CapacityBar, { props: { used: 12, total: 10 } })
    const fillBar = container.querySelector('[style*="width"]')
    expect(fillBar?.getAttribute('style')).toContain('width: 100%')
    expect(fillBar?.getAttribute('style')).toContain('var(--failed)')
  })
})
