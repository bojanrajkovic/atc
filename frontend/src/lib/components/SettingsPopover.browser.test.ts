import { fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
