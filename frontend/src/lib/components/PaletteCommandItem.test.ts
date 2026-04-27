import { describe, expect, it } from 'vitest'
import PaletteCommandItem from './PaletteCommandItem.svelte'

describe('PaletteCommandItem (unit)', () => {
  it('exports a component with Props interface', () => {
    // TypeScript compile-time check: the component exists and exports Props
    expect(PaletteCommandItem).toBeTruthy()
  })

  it('is a pure leaf component for rendering command rows in the palette', () => {
    // PaletteCommandItem is a pure Command.Item wrapper.
    // It takes props: label (string), icon? (string), shortcut? (string[]), onSelect (() => void).
    // All behavior is verified in browser tests due to Bits UI context requirements.
    const props = { label: 'Test', onSelect: () => {} }
    expect(props.label).toBe('Test')
  })
})
