import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import RunnerLabel from './RunnerLabel.svelte'

describe('RunnerLabel', () => {
  describe('renders summary with ⊞ prefix in mono font', () => {
    it('renders "runner-1" with ⊞ glyph in aria-hidden span', () => {
      const { container } = render(RunnerLabel, { props: { summary: 'runner-1' } })
      expect(screen.getByText('runner-1', { exact: false })).toBeTruthy()
      const glyphSpan = container.querySelector('[aria-hidden="true"]')
      expect(glyphSpan?.textContent).toBe('\u229E')
      const outerDiv = container.querySelector('.run-card-runner')
      expect(outerDiv?.className).toContain('font-mono')
    })
  })

  describe('renders multi-word summary with ⊞ prefix', () => {
    it('renders "3 runners" with ⊞ prefix', () => {
      const { container } = render(RunnerLabel, { props: { summary: '3 runners' } })
      expect(screen.getByText('3 runners', { exact: false })).toBeTruthy()
      const glyphSpan = container.querySelector('[aria-hidden="true"]')
      expect(glyphSpan?.textContent).toBe('\u229E')
    })
  })

  describe('null summary renders nothing', () => {
    it('renders no output when summary is null', () => {
      const { container } = render(RunnerLabel, { props: { summary: null } })
      expect(container.childElementCount).toBe(0)
    })
  })

  describe('long summary uses truncate class', () => {
    it('applies truncate class for overflow handling', () => {
      const { container } = render(RunnerLabel, {
        props: { summary: 'runner-with-a-very-long-name-12345' },
      })
      const outerDiv = container.querySelector('.run-card-runner')
      expect(outerDiv?.className).toContain('truncate')
    })
  })
})
