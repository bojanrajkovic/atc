import { expect, test } from '@playwright/test'

test.describe('App shell', () => {
  test('renders TopBar with logo and settings', async ({ page }) => {
    await page.goto('/')

    // Logo visible
    await expect(page.getByText('ATC')).toBeVisible()

    // Settings button visible
    await expect(page.getByRole('button', { name: /settings/i })).toBeVisible()

    // Connection indicator visible
    await expect(page.getByRole('status')).toBeVisible()
  })

  test('runner bar shows empty state without backend', async ({ page }) => {
    await page.goto('/')

    // RunnerBar shows the empty-state copy when no runners have reported.
    await expect(page.getByText('No active runners')).toBeVisible()
  })

  test('runner bar renders pool indicators with mock data', async ({ page }) => {
    // Mock WebSocket so ConnectionManager's connect() succeeds,
    // which triggers the /v1/state fetch
    await page.routeWebSocket('**/v1/ws', (ws) => {
      // Keep the WebSocket open — ConnectionManager proceeds to fetch state
      ws.onMessage(() => {
        // No-op: we don't need to respond to messages
      })
    })

    // Intercept /v1/state fetch and return mock pool data
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [],
          jobs: [],
          poolStats: [
            {
              labels: ['linux', 'x86_64'],
              queued: 2,
              running: 3,
              groupName: 'Default',
              isElastic: false,
              total: 10,
            },
            {
              labels: ['macos'],
              queued: 0,
              running: 1,
              groupName: 'macOS',
              isElastic: true,
              total: null,
            },
          ],
        }),
      })
    })
    await page.goto('/')
    // Wait for state fetch to resolve and stores to populate
    await expect(page.getByText('Default')).toBeVisible()
    await expect(page.getByText('macOS')).toBeVisible()
  })

  test('connection indicator shows connecting without backend', async ({ page }) => {
    await page.goto('/')

    const indicator = page.getByRole('status')
    await expect(indicator).toBeVisible()

    // Without a backend, ConnectionManager cycles connecting/reconnecting — never disconnected
    await expect(indicator).toHaveAttribute('aria-label', /connecting|reconnecting/i)
  })

  test('app shell fills full viewport height', async ({ page }) => {
    await page.goto('/')

    const viewportHeight = await page.evaluate(() => window.innerHeight)
    const shellHeight = await page.evaluate(() => {
      const shell = document.querySelector('[class*="h-dvh"]')
      return shell?.getBoundingClientRect().height ?? 0
    })

    expect(shellHeight).toBeCloseTo(viewportHeight, 0)
  })

  test('theme switching via popover updates document theme', async ({ page }) => {
    await page.goto('/')

    // Open settings popover
    await page.getByRole('button', { name: /settings/i }).click()
    await page.waitForTimeout(100)

    // Click warm theme
    await page.locator('button[aria-label="warm"]').click()
    await page.waitForTimeout(100)

    // Verify data-theme updated
    const dataTheme = await page.locator('html').getAttribute('data-theme')
    expect(dataTheme).toBe('warm')
  })
})
