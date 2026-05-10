import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { expect, test } from 'vitest'

// Must import app.css so the sibling-combinator backdrop suppression rule
// ([data-dialog-overlay] ~ [data-dialog-overlay] { display: none }) is live
// in document.styleSheets. Without this import the CSS rule is never applied
// and getComputedStyle assertions silently produce wrong results.
import '../../app.css'

import BackdropSuppressionHarness from './test-utils/BackdropSuppressionHarness.svelte'

test('sibling-combinator backdrop suppression hides the second overlay', async () => {
  render(BackdropSuppressionHarness)

  // Wait for Svelte reactivity + Bits UI portal/presence to settle.
  await tick()
  await new Promise<void>((r) => requestAnimationFrame(() => r()))

  // Both overlays portal to document.body in mount order: Sheet first, then
  // Command.Dialog. Neither has data-nested (Bits UI's DialogRootContext is
  // lexical Svelte context, not portal-runtime; sibling roots both have
  // parent === null). Use document.querySelectorAll — portals escape `container`.
  const overlays = Array.from(document.querySelectorAll<HTMLElement>('[data-dialog-overlay]'))

  expect(overlays.length).toBe(2)
  expect(getComputedStyle(overlays[0]!).display).not.toBe('none') // first overlay visible
  expect(getComputedStyle(overlays[1]!).display).toBe('none') // second hidden by ~
})
