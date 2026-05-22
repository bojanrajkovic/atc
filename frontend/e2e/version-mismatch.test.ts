import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { expect, test } from './lib/fixtures'
import { bigintReplacer, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * E2E smoke test for the version-mismatch banner (issue #47).
 *
 * The full wire-handshake flow happens through `eventDispatcher.dispatch()`
 * because the existing JS-level WS mock does not deliver `message` events
 * (Playwright/Vite delivery bug worked around in ws-mock.ts). Both
 * `ServerHello` and `GoingAway` frames are dispatched through the post-snapshot
 * dispatcher path, which is the same path live frames travel.
 */
test.describe('Version-mismatch banner (issue #47)', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)

    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(
          {
            lastSeq: 1n,
            runs: [],
            jobs: [],
            runnerPoolCapacities: [],
            displayTtlSeconds: 0,
          } satisfies StateSnapshot,
          bigintReplacer,
        ),
      })
    })

    await page.goto('/')
    // Wait for the connection to settle and the live region to mount.
    await expect(page.locator('[aria-label="Workflow run updates"]')).toBeAttached()
  })

  test('no banner on first ServerHello (session-reference is set silently)', async ({ page }) => {
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const dispatcher = (window as any).eventDispatcher
      dispatcher.dispatch({ kind: 'ServerHello', version: 'v1.0.0' })
    })

    // The banner role="status" container should not be present yet.
    await expect(page.getByRole('status', { name: /new build is available/i })).toHaveCount(0)
  })

  test('a second ServerHello with a different version shows the banner; Refresh now triggers reload', async ({
    page,
  }) => {
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const dispatcher = (window as any).eventDispatcher
      dispatcher.dispatch({ kind: 'ServerHello', version: 'v1.0.0' })
    })

    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const dispatcher = (window as any).eventDispatcher
      dispatcher.dispatch({ kind: 'ServerHello', version: 'v1.1.0' })
    })

    // Banner appears.
    const banner = page.getByRole('status', { name: /new build is available/i })
    await expect(banner).toBeVisible()

    // Reload spy: stub connectionStore.refreshNow before clicking the button
    // so the test page doesn't actually navigate away.
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const store = (window as any).__stores?.connectionStore
      if (!store)
        throw new Error('connectionStore bridge missing on window.__stores.connectionStore')
        // biome-ignore lint/suspicious/noExplicitAny: stubbing for the test
      ;(window as any).__refreshNowCalls = 0
      store.refreshNow = () => {
        // biome-ignore lint/suspicious/noExplicitAny: counter
        ;(window as any).__refreshNowCalls += 1
      }
    })

    await page.getByRole('button', { name: /refresh now/i }).click()

    const calls = await page.evaluate(
      // biome-ignore lint/suspicious/noExplicitAny: counter
      () => (window as any).__refreshNowCalls as number,
    )
    expect(calls).toBe(1)
  })

  test('GoingAway frame flips ConnectionIndicator to "Server restarting" framing', async ({
    page,
  }) => {
    // Drive the going-away envelope through the dispatcher, then simulate the
    // reconnecting status transition that follows the WS close in production.
    // (The mock WebSocket doesn't fire onclose from a JS-side handle, so we
    // toggle status directly via the dev bridge to keep this test focused on
    // the indicator wiring rather than the reconnect plumbing.)
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const dispatcher = (window as any).eventDispatcher
      dispatcher.dispatch({ kind: 'GoingAway', reason: 'server shutdown' })
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const store = (window as any).__stores?.connectionStore
      if (!store)
        throw new Error('connectionStore bridge missing on window.__stores.connectionStore')
      store.status = 'reconnecting'
    })

    // The indicator should now read "Server restarting…" via the connection
    // tooltip path.
    await expect(page.getByRole('status', { name: /server restarting/i })).toBeVisible()
  })
})
