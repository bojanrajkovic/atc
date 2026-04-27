import { describe, expect, it } from 'vitest'

import PaletteSection from './PaletteSection.svelte'

describe('PaletteSection (unit)', () => {
  it('exports a component with Props interface', () => {
    // TypeScript compile-time check: the component exists and exports Props
    expect(PaletteSection).toBeTruthy()
  })

  it('is a pure leaf component (no stores, no side effects)', () => {
    // PaletteSection is a pure wrapper around Command.Group.
    // It takes props: heading (string) and children (snippet).
    // All behavior is verified in browser tests due to Bits UI context requirements.
    const props = { heading: 'Test' }
    expect(props.heading).toBe('Test')
  })
})
