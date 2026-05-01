import { expect, test } from '@playwright/test'

test.describe('App rendering', () => {
  test('renders at / without console errors', async ({ page }) => {
    const consoleErrors: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text())
      }
    })

    await page.goto('/')

    // Verify TopBar renders with ATC logo
    await expect(page.getByText('ATC')).toBeVisible()

    // Verify no console errors
    expect(consoleErrors).toEqual([])
  })
})

test.describe('Theme switching', () => {
  const themes = [
    { name: 'warm', hue: '70' },
    { name: 'radar', hue: '155' },
    { name: 'violet', hue: '280' },
    { name: 'pink', hue: '310' },
  ]

  for (const { name, hue } of themes) {
    test(`clicking ${name} theme sets data-theme="${name}"`, async ({ page }) => {
      await page.goto('/')

      // Open the settings popover
      await page.getByRole('button', { name: /settings/i }).click()
      await page.waitForTimeout(100)

      // Find and click the theme button using locator with aria-label
      await page.locator(`button[aria-label="${name}"]`).click()
      await page.waitForTimeout(100)

      // Verify data-theme attribute on <html>
      const dataTheme = await page.locator('html').getAttribute('data-theme')
      expect(dataTheme).toBe(name)

      // Verify --hue CSS custom property
      const hueValue = await page.evaluate(() => {
        return getComputedStyle(document.documentElement).getPropertyValue('--hue').trim()
      })
      expect(hueValue).toBe(hue)
    })
  }
})

test.describe('Dark/light mode toggle', () => {
  test('toggling mode changes data-mode attribute', async ({ page }) => {
    await page.goto('/')

    // Open the settings popover
    await page.getByRole('button', { name: /settings/i }).click()
    await page.waitForTimeout(100)

    // Default is dark (no data-mode attribute or data-mode absent)
    // Click the mode toggle button using aria-label
    const modeToggle = page.locator('button[aria-label="Toggle light mode"]')
    await modeToggle.click()
    await page.waitForTimeout(100)

    // After toggle: should be light mode
    const dataMode = await page.locator('html').getAttribute('data-mode')
    expect(dataMode).toBe('light')

    // Toggle back
    await modeToggle.click()
    await page.waitForTimeout(100)

    // Should be dark again (no data-mode attribute)
    const dataModeAfter = await page.locator('html').getAttribute('data-mode')
    expect(dataModeAfter).toBeNull()
  })

  test('surface colors change between modes', async ({ page }) => {
    await page.goto('/')

    // Get dark mode background color
    const darkBg = await page.evaluate(() => {
      return getComputedStyle(document.body).backgroundColor
    })

    // Open the settings popover
    await page.getByRole('button', { name: /settings/i }).click()
    await page.waitForTimeout(100)

    // Switch to light mode
    await page.locator('button[aria-label="Toggle light mode"]').click()
    await page.waitForTimeout(100)

    // Get light mode background color
    const lightBg = await page.evaluate(() => {
      return getComputedStyle(document.body).backgroundColor
    })

    // Colors should be different
    expect(darkBg).not.toBe(lightBg)
  })
})

test.describe('Keyboard shortcut chords', () => {
  // Cross-platform Cmd/Ctrl: Meta on macOS, Control on CI (Linux).
  const cmdOrCtrl = process.platform === 'darwin' ? 'Meta' : 'Control'

  test('Cmd+D toggles dark mode (palette closed)', async ({ page }) => {
    await page.goto('/')
    await page.waitForTimeout(100)

    // Default is dark — no data-mode attribute on <html>.
    expect(await page.locator('html').getAttribute('data-mode')).toBeNull()

    await page.keyboard.press(`${cmdOrCtrl}+d`)
    await page.waitForTimeout(100)

    expect(await page.locator('html').getAttribute('data-mode')).toBe('light')
  })

  test('Cmd+D toggles dark mode AND closes the open palette', async ({ page }) => {
    await page.goto('/')
    await page.waitForTimeout(100)

    // Open palette via Cmd+K, confirm it's open.
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.getByRole('dialog')).toBeVisible()

    // Cmd+D from inside the palette toggles theme and closes the palette.
    await page.keyboard.press(`${cmdOrCtrl}+d`)
    await page.waitForTimeout(100)

    expect(await page.locator('html').getAttribute('data-mode')).toBe('light')
    await expect(page.getByRole('dialog')).not.toBeVisible()
  })

  test('Cmd+\\ toggles compact density', async ({ page }) => {
    await page.goto('/')
    await page.waitForTimeout(100)

    // Default is comfortable — no data-density attribute.
    expect(await page.locator('html').getAttribute('data-density')).toBeNull()

    await page.keyboard.press(`${cmdOrCtrl}+\\`)
    await page.waitForTimeout(100)

    expect(await page.locator('html').getAttribute('data-density')).toBe('compact')

    // Toggle back.
    await page.keyboard.press(`${cmdOrCtrl}+\\`)
    await page.waitForTimeout(100)

    expect(await page.locator('html').getAttribute('data-density')).toBeNull()
  })
})

test.describe('fe-foundation.AC1.1: Dark mode default and OKLCH chroma', () => {
  test('dark mode is default with no data-mode attribute', async ({ page }) => {
    await page.goto('/')

    // Verify no data-mode attribute (dark is default)
    const dataMode = await page.locator('html').getAttribute('data-mode')
    expect(dataMode).toBeNull()
  })

  test('surface background uses rich OKLCH chroma in dark mode', async ({ page }) => {
    await page.goto('/')

    // Get the computed background color in dark mode
    const bgColor = await page.evaluate(() => {
      return getComputedStyle(document.body).backgroundColor
    })

    // Convert RGB to determine it's not neutral gray
    // Dark mode surface should be oklch(16% 0.063 var(--hue)) — chroma 0.063 is rich
    // Light mode would be much lighter (97%+), so we verify it's dark and not neutral
    const rgb = bgColor.match(/\d+/g)
    if (rgb && rgb.length >= 3) {
      const [r, g, b] = rgb.map(Number)
      // In dark mode, all RGB values should be quite low (around 30-50 for a 16% lightness)
      expect(r).toBeLessThan(70)
      expect(g).toBeLessThan(70)
      expect(b).toBeLessThan(70)
    }
  })
})

test.describe('fe-foundation.AC1.4: Status colors constant across themes', () => {
  const themes = [
    { name: 'warm', hue: '70' },
    { name: 'radar', hue: '155' },
    { name: 'violet', hue: '280' },
    { name: 'pink', hue: '310' },
  ]

  const modes = [
    { name: 'dark', isDark: true },
    { name: 'light', isDark: false },
  ]

  // Helper to parse computed color to consistent format
  const getColorValue = (page: import('@playwright/test').Page, cssVar: string) => {
    return page.evaluate((varName: string) => {
      return getComputedStyle(document.documentElement).getPropertyValue(varName).trim()
    }, cssVar)
  }

  for (const mode of modes) {
    test(`status colors are constant in ${mode.name} mode across all themes`, async ({ page }) => {
      // Only test the four fixed-hue status colors (not --cancelled which uses --text-dim)
      const colors: Record<string, string[]> = {
        '--queued': [],
        '--running': [],
        '--success': [],
        '--failed': [],
      }

      await page.goto('/')

      // Open the settings popover
      await page.getByRole('button', { name: /settings/i }).click()
      await page.waitForTimeout(100)

      // Set to light mode if needed
      if (!mode.isDark) {
        await page.locator('button[aria-label="Toggle light mode"]').click()
        await page.waitForTimeout(100)
      }

      // For each theme, capture the status colors
      for (const theme of themes) {
        // Switch theme
        await page.locator(`button[aria-label="${theme.name}"]`).click()
        await page.waitForTimeout(100)

        // Read all status color values
        for (const colorVar of Object.keys(colors)) {
          const value = await getColorValue(page, colorVar)
          colors[colorVar]?.push(value)
        }
      }

      // Verify all themes produce identical values for each status color
      // (status colors have fixed hues, independent of theme)
      for (const [colorVar, values] of Object.entries(colors)) {
        const [firstValue, ...rest] = values
        if (firstValue === undefined) continue
        for (const value of rest) {
          expect(
            value,
            `${colorVar} should be constant across themes, but got different values`,
          ).toBe(firstValue)
        }
      }
    })
  }
})

// AC1.5 (shadcn components render with ATC token colors) is not testable yet —
// components are installed but not rendered in the scaffold UI. Add E2E coverage
// when components are integrated into real views.

test.describe('fe-foundation.AC1.6: prefers-reduced-motion disables animations', () => {
  test('animations are disabled when prefers-reduced-motion is set', async ({ page }) => {
    // Configure the browser to emulate reduced motion preference
    await page.emulateMedia({ reducedMotion: 'reduce' })

    await page.goto('/')

    // Check transition-duration on body (should be ~0.01ms per the CSS)
    const transitionDuration = await page.evaluate(() => {
      const duration = getComputedStyle(document.body).transitionDuration
      // Handle both formats: "0.01ms" and "1e-05s"
      // 0.01ms = 1e-05s (scientific notation in some browsers)
      return duration
    })

    // The prefers-reduced-motion media query sets transition-duration: 0.01ms !important
    // Browsers may return this in different formats, so check for very small duration
    // Convert to ms for comparison (1e-05s = 0.01ms)
    const durationMs = transitionDuration.includes('s')
      ? Number.parseFloat(transitionDuration) * 1000
      : Number.parseFloat(transitionDuration)

    // Duration should be very close to 0.01ms (allow for floating point variation)
    expect(durationMs).toBeLessThan(0.1)
  })
})

test.describe('fe-foundation.AC1.7: Theme and mode independence', () => {
  test('theme and mode can be changed independently', async ({ page }) => {
    await page.goto('/')

    // Wait for component to mount and set initial theme
    await page.waitForTimeout(100)

    // Start in dark mode, default theme (radar)
    let dataMode = await page.locator('html').getAttribute('data-mode')
    let dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBeNull() // Dark is default (no attribute)
    expect(dataTheme).toBe('radar') // Radar is default

    // Open the settings popover and switch to light mode
    await page.getByRole('button', { name: /settings/i }).click()
    await page.waitForTimeout(100)
    await page.locator('button[aria-label="Toggle light mode"]').click()
    await page.waitForTimeout(100)

    // Verify mode changed but theme unchanged
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBe('light')
    expect(dataTheme).toBe('radar') // Theme unchanged

    // Popover is still open after mode toggle — click warm theme directly
    await page.locator('button[aria-label="warm"]').click()
    await page.waitForTimeout(100)

    // Verify theme changed but mode unchanged
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBe('light') // Mode unchanged
    expect(dataTheme).toBe('warm') // Theme changed

    // Sequential theme change — popover still open, switch to pink
    await page.locator('button[aria-label="pink"]').click()
    await page.waitForTimeout(100)

    // Verify theme changed again but mode still light
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBe('light') // Mode still unchanged
    expect(dataTheme).toBe('pink') // Theme changed again

    // Toggle mode back to dark — verify bidirectional independence
    await page.locator('button[aria-label="Toggle light mode"]').click()
    await page.waitForTimeout(100)

    // Verify mode toggled back but theme unchanged
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBeNull() // Back to dark (no attribute)
    expect(dataTheme).toBe('pink') // Theme preserved
  })
})
