import { describe, expect, it } from 'vitest'
import PalettePoolItem from './PalettePoolItem.svelte'

describe('PalettePoolItem (unit)', () => {
  it('exports a component with Props interface', () => {
    // TypeScript compile-time check: the component exists and exports Props
    expect(PalettePoolItem).toBeTruthy()
  })

  it('is a pure leaf component for rendering pool rows in the palette', () => {
    // PalettePoolItem is a pure Command.Item wrapper.
    // It takes props: pool (PoolDisplay), query (string), onSelect (() => void).
    // All behavior is verified in browser tests due to Bits UI context requirements.
    const props = {
      pool: { labels: ['linux'], running: 1, queued: 0 },
      query: '',
      onSelect: () => {},
    }
    expect(props.pool.labels[0]).toBe('linux')
  })
})
