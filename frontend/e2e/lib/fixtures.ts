// Coverage-aware Playwright test fixtures.
//
// `@bgotink/playwright-coverage` hooks Page creation to call
// `page.coverage.startJSCoverage` (Chromium V8) before the first navigation
// and emits per-test attachments that the configured reporter merges into
// `coverage/e2e/`. Tests must import `test`/`expect` from this module — the
// stock `@playwright/test` exports skip the coverage hook.
//
// The reporter wiring lives in `playwright.config.ts`.
export { expect, test } from '@bgotink/playwright-coverage'
