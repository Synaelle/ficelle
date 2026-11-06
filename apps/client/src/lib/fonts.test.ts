// SPDX-License-Identifier: AGPL-3.0-or-later

import { builtInThemes } from '@app/ui/themes'
import { describe, expect, it } from 'vitest'
import bundled from '../styles/fonts.css?raw'
import { OPENDYSLEXIC } from './dyslexicFont'
import { describeFile } from './fonts'

describe('fonts', () => {
  it('reads a file name as its range, weight and style', () => {
    expect(describeFile('latin-700-italic')).toMatchObject({ weight: '700', style: 'italic' })
    expect(describeFile('latin-ext-400-normal')?.range).toContain('U+0100-02BA')
    expect(describeFile('latin-400-normal')?.range).toContain('U+0000-00FF')
    expect(describeFile('greek-400-normal')).toBeNull()
    expect(describeFile('latin-wght-italic', '200 900')).toMatchObject({
      weight: '200 900',
      style: 'italic',
    })
  })

  it("carries every built-in theme's fonts", () => {
    for (const theme of builtInThemes) {
      for (const family of Object.values(theme.fonts)) {
        expect(bundled, `${theme.id}: ${family}`).toContain(`font-family: '${family}'`)
      }
    }
  })

  it('asks only for files and ids the app accepts', () => {
    // the same rules as commands/fonts.rs
    expect(OPENDYSLEXIC.id).toMatch(/^[a-z0-9-]+(:vf)?$/)
    expect(OPENDYSLEXIC.id.endsWith(':vf')).toBe(!!OPENDYSLEXIC.weights)
    for (const name of OPENDYSLEXIC.files)
      expect(name).toMatch(/^latin(-ext)?-(400|500|700|wght)-(normal|italic)$/)
  })
})
