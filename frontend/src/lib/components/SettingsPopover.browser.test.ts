import { fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Must import app.css so the @theme inline color bridge (--color-input,
// --color-primary, etc.) and Tailwind v4 utility generation are live in
// document.styleSheets — vitest.config.browser.ts wires up @tailwindcss/vite
// for exactly this reason. Without it, utility classes silently no-op and
// the computed-style assertions below get false negatives.
import '../../app.css'

// Mock localStorage since browsers still need this mock in some contexts
const mockLocalStorage = (() => {
  let store: Record<string, string> = {}

  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value
    },
    removeItem: (key: string) => {
      delete store[key]
    },
    clear: () => {
      store = {}
    },
  }
})()

vi.stubGlobal('localStorage', mockLocalStorage)

describe('SettingsPopover (browser mode)', () => {
  // SettingsPopover uses uiStore, which is a module-level singleton with $effect.root().
  // We use vi.resetModules() to get a fresh singleton for each test.
  let uiStore: typeof import('$lib/stores/ui.svelte')['uiStore']
  let SettingsPopover: typeof import('./SettingsPopover.svelte').default

  beforeEach(async () => {
    mockLocalStorage.clear()
    vi.resetModules()
    const uiModule = await import('$lib/stores/ui.svelte')
    uiStore = uiModule.uiStore
    const componentModule = await import('./SettingsPopover.svelte')
    SettingsPopover = componentModule.default

    // vi.resetModules() does not re-evaluate ES modules in vitest's browser
    // pool — modules are cached at the browser/Vite layer, so the
    // module-level uiStore singleton persists across tests. Reset its state
    // explicitly so each test starts from defaults (matches the tests'
    // existing assumption that, e.g., mode === 'dark' initially).
    uiStore.theme = 'radar'
    uiStore.mode = 'dark'
    uiStore.density = 'comfortable'
    uiStore.activePoolFilter = null
    uiStore.selectedRunId = null
    uiStore.selectedJobId = null
    uiStore.lastTriggerRunId = null
    await new Promise((r) => setTimeout(r, 0))
  })

  afterEach(() => {
    mockLocalStorage.clear()
  })

  it('renders settings button with gear icon', () => {
    render(SettingsPopover)

    const button = screen.getByRole('button', { name: /settings/i })
    expect(button).toBeTruthy()

    // Check that button is visible (not hidden)
    const style = window.getComputedStyle(button)
    expect(style.display).not.toBe('none')
    expect(style.visibility).not.toBe('hidden')

    // Verify the button has a gear icon (SVG)
    const svg = button.querySelector('svg')
    expect(svg).toBeTruthy()
  })

  it('opens popover when settings button clicked', async () => {
    render(SettingsPopover)

    // Initially, theme controls should not be visible (not in DOM)
    expect(screen.queryByLabelText('Select theme')).toBeNull()
    expect(screen.queryByLabelText('Toggle light mode')).toBeNull()
    expect(screen.queryByLabelText('Toggle compact density')).toBeNull()

    // Click the settings button to open the popover
    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)

    // After click, the popover content should be visible
    // Wait a tick for the popover animation/render
    await new Promise((r) => setTimeout(r, 50))

    // Verify all three controls are now visible
    expect(screen.getByLabelText('Select theme')).toBeTruthy()
    expect(screen.getByLabelText('Toggle light mode')).toBeTruthy()
    expect(screen.getByLabelText('Toggle compact density')).toBeTruthy()
  })

  it('clicking theme dot updates uiStore.theme', async () => {
    render(SettingsPopover)

    // Open the popover
    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)
    await new Promise((r) => setTimeout(r, 50))

    // Initial theme should be 'radar'
    expect(uiStore.theme).toBe('radar')

    // Find and click the 'warm' theme toggle
    const themeToggles = screen.getAllByLabelText(/warm|radar|violet|pink/)
    const warmToggle = themeToggles.find((el) => el.getAttribute('aria-label') === 'warm')

    expect(warmToggle).toBeTruthy()
    await fireEvent.click(warmToggle!)
    await new Promise((r) => setTimeout(r, 0))

    expect(uiStore.theme).toBe('warm')
  })

  it('toggling mode updates uiStore.mode', async () => {
    render(SettingsPopover)

    // Open the popover
    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)
    await new Promise((r) => setTimeout(r, 50))

    // Initial mode should be 'dark'
    expect(uiStore.mode).toBe('dark')

    // Find and click the mode toggle
    const modeToggle = screen.getByLabelText('Toggle light mode')
    await fireEvent.click(modeToggle)
    await new Promise((r) => setTimeout(r, 0))

    expect(uiStore.mode).toBe('light')
  })

  it('toggling density updates uiStore.density', async () => {
    render(SettingsPopover)

    // Open the popover
    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)
    await new Promise((r) => setTimeout(r, 50))

    // Initial density should be 'comfortable'
    expect(uiStore.density).toBe('comfortable')

    // Find and click the density toggle
    const densityToggle = screen.getByLabelText('Toggle compact density')
    await fireEvent.click(densityToggle)
    await new Promise((r) => setTimeout(r, 0))

    expect(uiStore.density).toBe('compact')
  })

  // Regression checks for issue #31. The previous Toggle primitive was an
  // empty 28×28 transparent rectangle (no child content + bg-transparent),
  // functional but invisible to mouse/touch users. The Switch primitive
  // renders a filled track + thumb child by design.
  it('mode and density switches render a filled track and a sized thumb', async () => {
    render(SettingsPopover)

    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)
    await new Promise((r) => setTimeout(r, 50))

    const transparent = (color: string) => color === 'rgba(0, 0, 0, 0)' || color === 'transparent'

    for (const label of ['Toggle light mode', 'Toggle compact density']) {
      const track = screen.getByLabelText(label)

      // role=switch is the load-bearing semantic difference vs the previous
      // role=button Toggle — assistive tech announces this control as a
      // switch.
      expect(track.getAttribute('role')).toBe('switch')

      // Track must have a non-transparent background — the original bug was
      // bg-transparent + empty content. data-[state=unchecked]:bg-input fills
      // the track; if the @theme inline bridge ever regresses (as in PR #30)
      // this assertion catches it.
      const trackStyle = window.getComputedStyle(track)
      expect(transparent(trackStyle.backgroundColor)).toBe(false)

      // And the visible thumb child must have non-zero size, so even if some
      // future regression broke the track color, the thumb would still
      // signal interactivity.
      const thumb = track.querySelector('[data-slot="switch-thumb"]') as HTMLElement | null
      expect(thumb).not.toBeNull()
      const thumbRect = thumb!.getBoundingClientRect()
      expect(thumbRect.width).toBeGreaterThan(0)
      expect(thumbRect.height).toBeGreaterThan(0)
    }
  })

  it('flipping a switch toggles aria-checked and data-state', async () => {
    render(SettingsPopover)

    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)
    await new Promise((r) => setTimeout(r, 50))

    const modeSwitch = screen.getByLabelText('Toggle light mode')

    expect(modeSwitch.getAttribute('data-state')).toBe('unchecked')
    expect(modeSwitch.getAttribute('aria-checked')).toBe('false')

    await fireEvent.click(modeSwitch)
    await new Promise((r) => setTimeout(r, 0))

    expect(modeSwitch.getAttribute('data-state')).toBe('checked')
    expect(modeSwitch.getAttribute('aria-checked')).toBe('true')
  })

  it('reflects current theme as active in toggle group', async () => {
    // Set theme to 'violet' before rendering
    uiStore.theme = 'violet'
    await new Promise((r) => setTimeout(r, 0))

    render(SettingsPopover)

    // Open the popover
    const button = screen.getByRole('button', { name: /settings/i })
    await fireEvent.click(button)
    await new Promise((r) => setTimeout(r, 50))

    // Find the violet toggle
    const themeToggles = screen.getAllByLabelText(/warm|radar|violet|pink/)
    const violetToggle = themeToggles.find((el) => el.getAttribute('aria-label') === 'violet')

    expect(violetToggle).toBeTruthy()

    // The violet toggle should have the "pressed" state
    const isPressed =
      violetToggle!.getAttribute('aria-pressed') === 'true' ||
      violetToggle!.getAttribute('data-state') === 'on'

    expect(isPressed).toBe(true)
  })
})
