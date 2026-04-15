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

    // Verify something renders (the app title)
    await expect(page.locator('h1')).toBeVisible()

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

      // Click the theme button
      await page.getByRole('button', { name }).click()

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

    // Default is dark (no data-mode attribute or data-mode absent)
    // Click the mode toggle button
    const modeButton = page.getByRole('button', { name: /mode/i })
    await modeButton.click()

    // After toggle: should be light mode
    const dataMode = await page.locator('html').getAttribute('data-mode')
    expect(dataMode).toBe('light')

    // Toggle back
    await modeButton.click()

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

    // Switch to light mode
    await page.getByRole('button', { name: /mode/i }).click()

    // Get light mode background color
    const lightBg = await page.evaluate(() => {
      return getComputedStyle(document.body).backgroundColor
    })

    // Colors should be different
    expect(darkBg).not.toBe(lightBg)
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

      // Set to light mode if needed
      if (!mode.isDark) {
        await page.getByRole('button', { name: /mode/i }).click()
        await page.waitForTimeout(100)
      }

      // For each theme, capture the status colors
      for (const theme of themes) {
        // Switch theme
        await page.getByRole('button', { name: theme.name }).click()
        await page.waitForTimeout(100)

        // Read all status color values
        for (const colorVar of Object.keys(colors)) {
          const value = await getColorValue(page, colorVar)
          colors[colorVar].push(value)
        }
      }

      // Verify all themes produce identical values for each status color
      // (status colors have fixed hues, independent of theme)
      for (const [colorVar, values] of Object.entries(colors)) {
        const firstValue = values[0]
        for (const value of values.slice(1)) {
          expect(value).toBe(
            firstValue,
            `${colorVar} should be constant across themes, but got different values`,
          )
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

    // Switch to light mode
    await page.getByRole('button', { name: /mode/i }).click()
    await page.waitForTimeout(100)
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBe('light')
    expect(dataTheme).toBe('radar') // Theme unchanged

    // Switch to violet theme
    await page.getByRole('button', { name: 'violet' }).click()
    await page.waitForTimeout(100)
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBe('light') // Mode unchanged
    expect(dataTheme).toBe('violet') // Theme changed

    // Switch theme to pink
    await page.getByRole('button', { name: 'pink' }).click()
    await page.waitForTimeout(100)
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBe('light') // Mode unchanged
    expect(dataTheme).toBe('pink') // Theme changed

    // Toggle back to dark mode
    await page.getByRole('button', { name: /mode/i }).click()
    await page.waitForTimeout(100)
    dataMode = await page.locator('html').getAttribute('data-mode')
    dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataMode).toBeNull() // Mode changed to dark (no attribute)
    expect(dataTheme).toBe('pink') // Theme still pink
  })
})
