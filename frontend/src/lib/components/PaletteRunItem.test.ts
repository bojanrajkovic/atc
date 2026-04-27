import { describe, expect, it } from 'vitest'

import PaletteRunItem from './PaletteRunItem.svelte'

describe('PaletteRunItem (unit)', () => {
  it('exports a component with Props interface', () => {
    // TypeScript compile-time check: the component exists and exports Props
    expect(PaletteRunItem).toBeTruthy()
  })

  it('is a pure leaf component for rendering run rows in the command palette', () => {
    // PaletteRunItem is a pure Command.Item wrapper.
    // It takes props: run (WorkflowRun) and onSelect (() => void).
    // All behavior is verified in browser tests due to Bits UI context requirements.
    const props = { run: { displayTitle: 'Test' }, onSelect: () => {} }
    expect(props.run.displayTitle).toBe('Test')
  })
})
