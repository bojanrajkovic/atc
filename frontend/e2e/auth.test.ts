import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { expect, test } from './lib/fixtures'
import { bigintReplacer, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

const connectedSnapshot: StateSnapshot = {
  lastSeq: 0n,
  runs: [],
  jobs: [],
  runnerPoolCapacities: [],
  displayTtlSeconds: 0,
}

/**
 * E2E smoke tests for the login screen and identity chrome (#463). The
 * 401-detection state machine itself is unit-tested exhaustively in
 * connection.auth.test.ts; these tests only cover the rendering wiring —
 * that the right DOM shows up for each server response shape.
 */
test.describe('Login screen and identity chrome', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  test('shows the login screen when /v1/state 401s with auth_required', async ({ page }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        status: 401,
        contentType: 'application/json',
        body: JSON.stringify({ reason: 'auth_required' }),
      })
    })
    // return_to is computed at click time (see LoginScreen.svelte), not baked
    // into a static href — intercept the resulting navigation to verify it.
    let loginRequestUrl: string | null = null
    await page.route('**/v1/auth/github/login**', (route) => {
      loginRequestUrl = route.request().url()
      route.fulfill({ status: 200, contentType: 'text/html', body: 'ok' })
    })

    await page.goto('/')

    const link = page.getByRole('link', { name: /sign in with github/i })
    await expect(link).toBeVisible()

    // The normal dashboard shell must not render behind/alongside the login screen.
    await expect(page.locator('[data-runner-bar]')).toHaveCount(0)

    await link.click()
    await expect.poll(() => loginRequestUrl).toMatch(/\/v1\/auth\/github\/login\?return_to=/)
  })

  test('shows identity chrome once /v1/auth/me resolves; logout posts to the logout endpoint', async ({
    page,
  }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(connectedSnapshot, bigintReplacer),
      })
    })
    await page.route('**/v1/auth/me', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          login: 'octocat',
          repoCount: 5,
          reposRefreshedAt: '2026-07-04T00:00:00Z',
          stale: false,
        }),
      })
    })
    let logoutRequested = false
    await page.route('**/v1/auth/github/logout', (route) => {
      logoutRequested = true
      route.fulfill({ status: 204 })
    })

    await page.goto('/')

    await expect(page.getByText('octocat')).toBeVisible()
    const logoutButton = page.getByRole('button', { name: /log out/i })
    await expect(logoutButton).toBeVisible()

    await logoutButton.click()
    await expect.poll(() => logoutRequested).toBe(true)
  })

  test('mode=none (/v1/auth/me 404s, not mounted): no login screen, no identity chrome', async ({
    page,
  }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(connectedSnapshot, bigintReplacer),
      })
    })
    // mode=none never mounts /v1/auth/me at all — the real server 404s.
    await page.route('**/v1/auth/me', (route) => {
      route.fulfill({ status: 404 })
    })

    await page.goto('/')

    await expect(page.locator('[data-runner-bar]')).toBeVisible()
    await expect(page.getByRole('link', { name: /sign in with github/i })).toHaveCount(0)
    await expect(page.getByRole('button', { name: /log out/i })).toHaveCount(0)
  })
})
