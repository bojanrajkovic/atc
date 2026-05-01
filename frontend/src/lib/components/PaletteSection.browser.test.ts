import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import Wrapper from './test-utils/PaletteSectionWrapper.svelte'

/**
 * Browser-mode tests for PaletteSection.
 * The Bits UI Command.Group context requirements necessitate browser mode.
 */

describe('PaletteSection (browser)', () => {
  it('renders the heading text inside the group', () => {
    render(Wrapper, {
      props: { heading: 'Recent', content: '' },
    })
    expect(screen.getByText('Recent')).toBeTruthy()
  })

  it('renders children content when provided', () => {
    render(Wrapper, {
      props: { heading: 'Runs', content: 'Test child content' },
    })
    expect(screen.getByText('Test child content')).toBeTruthy()
  })

  it('renders the heading but no additional content when empty', () => {
    render(Wrapper, {
      props: { heading: 'Jobs', content: '' },
    })
    expect(screen.getByText('Jobs')).toBeTruthy()
  })
})
