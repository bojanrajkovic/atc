/**
 * Observes a write to `window.location.href` without breaking anything else
 * that reads `window.location` during the same test (e.g. a relative
 * `fetch()` call resolving against the real origin). A setter alone shadows
 * `href` with `undefined`, which is why this needs a getter too — see the
 * call sites for the specific relative-URL resolution this protects.
 */
export async function withLocationHrefSpy(
  run: () => Promise<unknown> | undefined,
): Promise<string | null> {
  let hrefSet: string | null = null
  const originalLocation = window.location
  Object.defineProperty(window, 'location', {
    configurable: true,
    value: {
      ...originalLocation,
      get href() {
        return hrefSet ?? originalLocation.href
      },
      set href(v: string) {
        hrefSet = v
      },
    },
  })
  try {
    await run()
    return hrefSet
  } finally {
    Object.defineProperty(window, 'location', { configurable: true, value: originalLocation })
  }
}
