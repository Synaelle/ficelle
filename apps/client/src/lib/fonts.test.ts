// SPDX-License-Identifier: AGPL-3.0-or-later

import { builtInThemes } from '@app/ui/themes'
import { describe, expect, it } from 'vitest'
import bundled from '../styles/fonts.css?raw'
import webOnly from '../styles/fonts-themes.css?raw'
import { OPENDYSLEXIC } from './dyslexicFont'
import { describeFile, familyOf, FONT_LIST, quoted, THEME_FONTS, themeFontPack } from './fonts'

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

  it("has every built-in theme's fonts, bundled or as packs, and the web pages all", () => {
    const web = bundled + webOnly
    for (const theme of builtInThemes) {
      for (const family of Object.values(theme.fonts)) {
        const inApp = bundled.includes(`font-family: '${family}'`)
        expect(inApp || themeFontPack(family), `${theme.id}: ${family}`).toBeTruthy()
        expect(inApp && themeFontPack(family), `${family} twice`).toBeFalsy()
        expect(web, family).toContain(`font-family: '${family}'`)
      }
    }
  })

  it('asks only for files and ids the app accepts', () => {
    // the same rules as commands/fonts.rs
    for (const pack of [...THEME_FONTS, OPENDYSLEXIC]) {
      expect(pack.id).toMatch(/^[a-z0-9-]+(:vf)?$/)
      expect(pack.id.endsWith(':vf')).toBe(!!pack.weights)
      for (const name of pack.files)
        expect(name).toMatch(/^latin(-ext)?-(400|500|700|wght)-(normal|italic)$/)
    }
  })

  it('names the family a choice puts in css, quoted safely', () => {
    expect(familyOf({ source: 'list', id: 'jetbrains-mono' })).toBe('JetBrains Mono')
    expect(familyOf({ source: 'list', id: 'gone' })).toBeNull()
    expect(familyOf({ source: 'device', family: 'Segoe UI' })).toBe('Segoe UI')
    expect(quoted('Segoe UI')).toBe('"Segoe UI"')
    expect(quoted('a"b\\c')).toBe('"a\\"b\\\\c"')
  })
})
