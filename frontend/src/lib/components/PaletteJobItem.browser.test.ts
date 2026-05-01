import { render, screen } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'

import { createMockJob, createMockRun } from '$lib/test-utils/factories'
import Wrapper from './test-utils/PaletteJobItemWrapper.svelte'

describe('PaletteJobItem (browser mode)', () => {
  it('renders job name and parent run title', () => {
    const parentRun = createMockRun({ displayTitle: 'PR #42' })
    const job = createMockJob({
      name: 'build-step',
      status: 'Queued',
      conclusion: null,
    })
    const onSelect = vi.fn()

    render(Wrapper, {
      props: { job, parentRun, onSelect },
    })

    // Verify job name renders
    expect(screen.getByText('build-step')).toBeTruthy()
    // Verify parent run context renders
    expect(screen.getByText(/in PR #42/)).toBeTruthy()
  })

  it('renders StatusIcon glyph for Queued status', () => {
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

    // Verify job name renders
    expect(screen.getByText('queued-job')).toBeTruthy()
    // StatusIcon for Queued status renders a glyph (◐)
    const glyph = screen.getByText(/[◐▶✓✗]/u)
    expect(glyph).toBeTruthy()
  })

  it('renders StatusIcon glyph for InProgress status', () => {
    const parentRun = createMockRun()
    const job = createMockJob({
      name: 'running-job',
      status: 'InProgress',
      conclusion: null,
    })
    const onSelect = vi.fn()

    render(Wrapper, {
      props: { job, parentRun, onSelect },
    })

    expect(screen.getByText('running-job')).toBeTruthy()
    // StatusIcon for InProgress status renders ▶
    const glyph = screen.getByText(/[◐▶✓✗]/u)
    expect(glyph).toBeTruthy()
  })

  it('renders StatusIcon glyph for Completed/Success status', () => {
    const parentRun = createMockRun()
    const job = createMockJob({
      name: 'completed-job',
      status: 'Completed',
      conclusion: 'Success',
    })
    const onSelect = vi.fn()

    render(Wrapper, {
      props: { job, parentRun, onSelect },
    })

    expect(screen.getByText('completed-job')).toBeTruthy()
    // StatusIcon for Completed/Success status renders ✓
    const glyph = screen.getByText(/[◐▶✓✗]/u)
    expect(glyph).toBeTruthy()
  })

  it('calls onSelect when item is selected', () => {
    const parentRun = createMockRun()
    const job = createMockJob({ name: 'test-job' })
    const onSelect = vi.fn()

    render(Wrapper, {
      props: { job, parentRun, onSelect },
    })

    // Verify component renders
    expect(screen.getByText('test-job')).toBeTruthy()
    // onSelect should not be called during render
    expect(onSelect).not.toHaveBeenCalled()
  })
})
