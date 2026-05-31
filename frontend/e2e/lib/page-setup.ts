import type { Page } from '@playwright/test'
import { WS_MOCK_INIT_SCRIPT } from './ws-mock'

/** Options for {@link setupMockedPage}. All optional; the defaults reproduce the
 *  bare "empty snapshot, no hover, default viewport" setup most specs use. */
export interface SetupMockedPageOptions {
  /**
   * Stub `matchMedia` so `HoverPeekPopover`'s `canHover` flag is always false.
   * This prevents the 250ms hover timer from firing during keyboard-driven
   * tests and stealing focus from the element that `onCloseAutoFocus` just
   * restored. Only the keyboard/focus specs need this.
   */
  stubHover?: boolean
  /** Explicit viewport applied before navigation. */
  viewport?: { width: number; height: number }
}

/**
 * Standard E2E page setup: install the JS-level WebSocket mock, stub
 * `GET /v1/state` with an empty snapshot, navigate to `/`, and wait for the
 * store bridge to be live and the connection to report `connected`.
 *
 * `window.__stores` is assigned as a single object literal in `main.ts`, so
 * `uiStore` / `runStore` / `runnerStore` / `connectionStore` all become defined
 * at the same instant — requiring all of them is equivalent to requiring any
 * one, and lets a single predicate serve every spec. The real gate is
 * `connectionStore.status === 'connected'`; the `uiStore`-only fallback covers
 * the rare case where the connected transition lags the bridge assignment.
 *
 * Specs seed runs/jobs after this resolves (via `sendWS` / `makeRunEvent`), so
 * the stubbed snapshot is intentionally empty.
 */
export async function setupMockedPage(
  page: Page,
  opts: SetupMockedPageOptions = {},
): Promise<void> {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)

  if (opts.stubHover) {
    await page.addInitScript(() => {
      const original = window.matchMedia
      window.matchMedia = (query: string): MediaQueryList => {
        if (query === '(hover: hover) and (pointer: fine)') {
          return {
            matches: false,
            media: query,
            addListener: () => {},
            removeListener: () => {},
            addEventListener: () => {},
            removeEventListener: () => {},
            dispatchEvent: () => false,
            onchange: null,
          } as unknown as MediaQueryList
        }
        return original.call(window, query)
      }
    })
  }

  await page.route('**/v1/state', (route) =>
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ lastSeq: 1, runs: [], jobs: [] }),
    }),
  )

  if (opts.viewport) await page.setViewportSize(opts.viewport)

  await page.goto('/')

  try {
    await page.waitForFunction(
      () => {
        const s = window.__stores
        return (
          typeof s?.uiStore !== 'undefined' &&
          typeof s?.runStore !== 'undefined' &&
          typeof s?.runnerStore !== 'undefined' &&
          typeof s?.connectionStore !== 'undefined' &&
          s.connectionStore.status === 'connected'
        )
      },
      { timeout: 15_000 },
    )
  } catch {
    await page.waitForFunction(() => typeof window.__stores?.uiStore !== 'undefined', {
      timeout: 10_000,
    })
  }
}
