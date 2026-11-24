// SPDX-License-Identifier: AGPL-3.0-or-later

import { builtInThemes } from '@app/ui/themes'
import { describe, expect, it } from 'vitest'
import commands from '../../src-tauri/src/commands/fonts.rs?raw'
import bundled from '../styles/fonts.css?raw'
import { FONT_PACKS } from './chosenFonts.svelte'
import { OPENDYSLEXIC } from './dyslexicFont'
import { describeFile } from './fonts'

describe('fonts', () => {
  it('reads a file name as its range, weight and style', () => {
    expect(describeFile('latin-700-italic')).toMatchObject({ weight: '700', style: 'italic' })
    expect(describeFile('latin-ext-400-normal')?.range).toContain('U+0100-02BA')
    expect(describeFile('latin-400-normal')?.range).toContain('U+0000-00FF')
    expect(describeFile('greek-400-normal')).toBeNull()
    expect(describeFile('latin-wght-italic')).toBeNull()
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
    for (const pack of [OPENDYSLEXIC, ...Object.values(FONT_PACKS)]) {
      expect(pack.id).toMatch(/^[a-z0-9-]+$/)
      for (const name of pack.files)
        expect(name).toMatch(/^latin(-ext)?-(400|700)-(normal|italic)$/)
    }
  })

  it('asks only for files the app knows the hash of, so none is refused', () => {
    for (const pack of [OPENDYSLEXIC, ...Object.values(FONT_PACKS)]) {
      for (const name of pack.files) {
        const entry = new RegExp(`"${pack.id}",\\s*"${name}",\\s*"[0-9a-f]{64}"`)
        expect(commands, `${pack.id} ${name}`).toMatch(entry)
      }
    }
  })
})
