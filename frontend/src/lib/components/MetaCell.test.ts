import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import MetaCell from './MetaCell.svelte'

describe('MetaCell', () => {
  it('interactivity.AC2.1 renders label and value text when value is provided', () => {
    render(MetaCell, { props: { label: 'Commit', value: 'abc1234' } })

    expect(screen.getByText('Commit')).toBeTruthy()
    expect(screen.getByText('abc1234')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders default placeholder — when value is null', () => {
    render(MetaCell, { props: { label: 'Commit', value: null } })

    expect(screen.getByText('—')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders default placeholder — when value is undefined', () => {
    render(MetaCell, { props: { label: 'Event', value: undefined } })

    expect(screen.getByText('—')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders default placeholder — when value is empty string', () => {
    render(MetaCell, { props: { label: 'Runner', value: '' } })

    expect(screen.getByText('—')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders custom placeholder when value is null and placeholder is provided', () => {
    render(MetaCell, { props: { label: 'Duration', value: null, placeholder: 'n/a' } })

    expect(screen.getByText('n/a')).toBeTruthy()
  })
})
