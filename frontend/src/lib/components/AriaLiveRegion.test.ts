import { flushSync, mount, unmount } from 'svelte'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { liveRegion } from '$lib/aria/live-region.svelte'
import AriaLiveRegion from './AriaLiveRegion.svelte'

describe('AriaLiveRegion component', () => {
  let host: HTMLElement
  let component: ReturnType<typeof mount>

  beforeEach(() => {
    liveRegion.message = ''
    liveRegion.busy = false
    host = document.createElement('div')
    document.body.appendChild(host)
    component = mount(AriaLiveRegion, { target: host })
  })

  afterEach(async () => {
    await unmount(component)
    host.remove()
    liveRegion.message = ''
    liveRegion.busy = false
  })

  function getDiv(): HTMLElement {
    const el = host.querySelector('div')
    if (!el) throw new Error('AriaLiveRegion div not found')
    return el
  }

  it('AC6.1 — renders a single div with role="status"', () => {
    const el = getDiv()
    expect(el.getAttribute('role')).toBe('status')
  })

  it('AC6.1 — aria-live="polite"', () => {
    const el = getDiv()
    expect(el.getAttribute('aria-live')).toBe('polite')
  })

  it('AC6.1 — aria-atomic="true"', () => {
    const el = getDiv()
    expect(el.getAttribute('aria-atomic')).toBe('true')
  })

  it('AC6.1 — initial aria-busy="false" (explicit string, not bare attribute)', () => {
    const el = getDiv()
    expect(el.getAttribute('aria-busy')).toBe('false')
  })

  it('AC6.1 — aria-label="Workflow run updates"', () => {
    const el = getDiv()
    expect(el.getAttribute('aria-label')).toBe('Workflow run updates')
  })

  it('AC6.1 — has sr-only class', () => {
    const el = getDiv()
    expect(el.classList.contains('sr-only')).toBe(true)
  })

  it('aria-busy flips to "true" when liveRegion.busy is set to true', () => {
    liveRegion.busy = true
    flushSync()
    const el = getDiv()
    expect(el.getAttribute('aria-busy')).toBe('true')
  })

  it('aria-busy flips back to "false" when liveRegion.busy is set to false', () => {
    liveRegion.busy = true
    flushSync()
    liveRegion.busy = false
    flushSync()
    const el = getDiv()
    expect(el.getAttribute('aria-busy')).toBe('false')
  })

  it('textContent reflects liveRegion.message', () => {
    liveRegion.message = 'Run deploy-job for acme/api (push) queued'
    flushSync()
    const el = getDiv()
    expect(el.textContent).toBe('Run deploy-job for acme/api (push) queued')
  })

  it('textContent updates when liveRegion.message changes', () => {
    liveRegion.message = 'first message'
    flushSync()
    liveRegion.message = 'second message'
    flushSync()
    const el = getDiv()
    expect(el.textContent).toBe('second message')
  })

  it('empty message renders empty textContent', () => {
    const el = getDiv()
    expect(el.textContent).toBe('')
  })
})
