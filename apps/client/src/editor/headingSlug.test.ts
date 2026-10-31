// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import { headingSlug } from './headingSlug'

describe('heading anchors', () => {
  it('match the ones GitHub makes', () => {
    expect(headingSlug('Getting Started')).toBe('getting-started')
    expect(headingSlug('What is this? (and why)')).toBe('what-is-this-and-why')
    expect(headingSlug('Été à Paris')).toBe('été-à-paris')
    expect(headingSlug('snake_case and dash-ed')).toBe('snake_case-and-dash-ed')
    expect(headingSlug('Two  spaces')).toBe('two--spaces')
  })
})
