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
