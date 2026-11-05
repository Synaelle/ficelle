// SPDX-License-Identifier: AGPL-3.0-or-later

import { builtInThemes } from '@app/ui/themes'
import { describe, expect, it } from 'vitest'
import bundled from '../styles/fonts.css?raw'
import { OPENDYSLEXIC } from './dyslexicFont'
import { describeFile, familyOf, FONT_LIST, quoted } from './fonts'

describe('fonts', () => {
  it('lists fonts with plain, unique ids the app accepts', () => {
    const ids = FONT_LIST.map((font) => font.id)
    expect(new Set(ids).size).toBe(ids.length)
    // the same rule as commands/fonts.rs
    for (const id of ids) expect(id).toMatch(/^[a-z0-9-]{1,64}$/)
    expect(FONT_LIST.filter((font) => font.kind === 'mono').length).toBeGreaterThan(5)
  })

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

  it('names the family a choice puts in css, quoted safely', () => {
    expect(familyOf({ source: 'list', id: 'jetbrains-mono' })).toBe('JetBrains Mono')
    expect(familyOf({ source: 'list', id: 'gone' })).toBeNull()
    expect(familyOf({ source: 'device', family: 'Segoe UI' })).toBe('Segoe UI')
    expect(quoted('Segoe UI')).toBe('"Segoe UI"')
    expect(quoted('a"b\\c')).toBe('"a\\"b\\\\c"')
  })
})
