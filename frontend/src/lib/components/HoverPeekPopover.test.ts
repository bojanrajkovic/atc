import { render, screen } from '@testing-library/svelte'
import { afterAll, describe, expect, it } from 'vitest'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRun } from '$lib/test-utils/factories'
import HoverPeekPopover from './HoverPeekPopover.svelte'

// Clean up uiStore timer after all tests
afterAll(() => {
  uiStore.destroy()
})

// Anchor element for the popover — provide a real DOM element so
// bits-ui has a positioning reference. Append to body so it's in-document.
function makeAnchor(): HTMLElement {
  const el = document.createElement('div')
  document.body.appendChild(el)
  return el
}

describe('HoverPeekPopover', () => {
  it('renders status label in the Status row when open=true', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'Queued' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'Queued',
        totalJobs: 3,
        stepsCompleted: 2,
        stepsTotal: 10,
        durationText: '1:23',
        runnerSummary: 'ubuntu-latest',
        anchor,
        open: true,
      },
    })

    // Status label renders inside .status-label (value cell of Status row).
    // The popover is portal-rendered to document.body so we query the full document.
    const statusLabelEl = document.querySelector('.status-label')
    expect(statusLabelEl).toBeTruthy()
    expect(statusLabelEl?.textContent?.trim()).toBe('Queued')
  })

  it('renders title row with run displayTitle', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'InProgress', displayTitle: 'My workflow run' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'In progress',
        totalJobs: 2,
        stepsCompleted: 1,
        stepsTotal: 4,
        durationText: '0:45',
        runnerSummary: null,
        anchor,
        open: true,
      },
    })

    expect(screen.getByText('My workflow run')).toBeTruthy()
  })

  it('renders Steps complete row as "{stepsCompleted}/{stepsTotal}"', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'InProgress' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'In progress',
        totalJobs: 5,
        stepsCompleted: 7,
        stepsTotal: 20,
        durationText: '2:45',
        runnerSummary: null,
        anchor,
        open: true,
      },
    })

    // Label "Steps complete" and value "7/20" render as separate elements
    expect(screen.getByText('Steps complete')).toBeTruthy()
    expect(screen.getByText('7/20')).toBeTruthy()
  })

  it('renders Duration row with durationText value', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'InProgress' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'In progress',
        totalJobs: 2,
        stepsCompleted: 1,
        stepsTotal: 4,
        durationText: '3:15',
        runnerSummary: null,
        anchor,
        open: true,
      },
    })

    // Label "Duration" and value "3:15" render as separate elements
    expect(screen.getByText('Duration')).toBeTruthy()
    expect(screen.getByText('3:15')).toBeTruthy()
  })

  it('renders Runner row when runnerSummary is non-null', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'InProgress' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'In progress',
        totalJobs: 2,
        stepsCompleted: 0,
        stepsTotal: 4,
        durationText: '0:45',
        runnerSummary: '2 runners',
        anchor,
        open: true,
      },
    })

    expect(screen.getByText('Runner')).toBeTruthy()
    expect(screen.getByText('2 runners')).toBeTruthy()
  })

  it('omits Runner row when runnerSummary is null', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'Queued' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'Queued',
        totalJobs: 1,
        stepsCompleted: 0,
        stepsTotal: 2,
        durationText: '0:10',
        runnerSummary: null,
        anchor,
        open: true,
      },
    })

    expect(screen.queryByText('Runner')).toBeNull()
  })

  it('renders keyboard hint footer', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'Queued' })
    render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'Queued',
        totalJobs: 1,
        stepsCompleted: 0,
        stepsTotal: 2,
        durationText: '0:10',
        runnerSummary: null,
        anchor,
        open: true,
      },
    })

    const hint = document.querySelector('.peek-hint')
    expect(hint).toBeTruthy()
    expect(hint?.textContent).toContain('Click for full panel')
    expect(hint?.textContent).toContain('Enter')
  })

  it('popover content is portal-rendered (not inside mount container)', () => {
    const anchor = makeAnchor()
    const run = createMockRun({ status: 'InProgress' })
    const { container } = render(HoverPeekPopover, {
      props: {
        run,
        statusLabel: 'In progress',
        totalJobs: 2,
        stepsCompleted: 3,
        stepsTotal: 8,
        durationText: '1:00',
        runnerSummary: null,
        anchor,
        open: true,
      },
    })

    // The popover content has class hover-peek-popover and is in the document
    const popoverEl = document.querySelector('.hover-peek-popover')
    expect(popoverEl).toBeTruthy()

    // The mount container should NOT contain the popover content —
    // Bits UI portals it to document.body directly.
    expect(container.querySelector('.hover-peek-popover')).toBeNull()
  })
})
