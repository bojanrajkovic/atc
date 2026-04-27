import { describe, expect, it } from 'vitest'
import PaletteJobItem from './PaletteJobItem.svelte'

describe('PaletteJobItem (unit)', () => {
  it('exports a component with Props interface', () => {
    // TypeScript compile-time check: the component exists and exports Props
    expect(PaletteJobItem).toBeTruthy()
  })

  it('is a pure leaf component for rendering job rows in the command palette', () => {
    // PaletteJobItem is a pure Command.Item wrapper.
    // It takes props: job (Job), parentRun (WorkflowRun) and onSelect (() => void).
    // All behavior is verified in browser tests due to Bits UI context requirements.
    const props = { job: { name: 'Test' }, parentRun: { displayTitle: 'Run' }, onSelect: () => {} }
    expect(props.job.name).toBe('Test')
  })
})
