// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { tagShort } from './tagMark.svelte'

describe('tagShort', () => {
  it('shows the last level when it says which tag it is', () => {
    expect(tagShort('Projects/Garden', ['Projects/Garden', 'Seeds'])).toBe('Garden')
  })

  it('shows as many levels as tell two tags apart', () => {
    const tags = ['Paperwork/insurance', 'admin/insurance', 'a/b/c', 'x/b/c']
    expect(tagShort('Paperwork/insurance', tags)).toBe('Paperwork/insurance')
    expect(tagShort('admin/insurance', tags)).toBe('admin/insurance')
    expect(tagShort('a/b/c', tags)).toBe('a/b/c')
    expect(tagShort('insurance', ['insurance', 'admin/insurance'])).toBe('insurance')
    expect(tagShort('admin/insurance', ['insurance', 'admin/insurance'])).toBe('admin/insurance')
  })
})
