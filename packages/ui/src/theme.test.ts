// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import { builtInThemes, DEFAULT_THEME_ID } from '../themes'
import darkroomJson from '../themes/darkroom.json' with { type: 'json' }
import { contrastRatio } from './color'
import {
  MIN_TEXT_CONTRAST,
  pureBlack,
  readabilityProblems,
  themeToCssVars,
  type PaletteKey,
  type Theme,
} from './theme'

const darkroom = darkroomJson as Theme

describe('the theme', () => {
  it('is the default and reads well in both modes', () => {
    expect(builtInThemes.map((t) => t.id)).toContain(DEFAULT_THEME_ID)
    expect(readabilityProblems(darkroom.colors.dark)).toEqual([])
    expect(readabilityProblems(darkroom.colors.light)).toEqual([])
  })

  it('names what is too hard to read', () => {
    const light = { ...darkroom.colors.light, text: darkroom.colors.light.bg }
    expect(readabilityProblems(light).join(' ')).toContain('text on bg')
  })
})

describe('themeToCssVars', () => {
  it('exposes colors, sizes and motion as css variables', () => {
    const vars = themeToCssVars(darkroom, 'light')
    expect(vars['--color-accent']).toBe(darkroom.colors.light.accent)
    expect(vars['--color-text-muted']).toBe(darkroom.colors.light.textMuted)
    expect(vars['--color-surface2']).toBeUndefined()
    expect(vars['--color-surface-2']).toBe(darkroom.colors.light.surface2)
    expect(vars['--space-7']).toBe('48px')
    expect(vars['--dur-fast']).toBe('140ms')
  })

  it('has one small corner everywhere', () => {
    const vars = themeToCssVars(darkroom, 'dark')
    expect(vars['--radius-control']).toBe('2px')
    expect(vars['--radius-popover']).toBe('2px')
    expect(vars['--radius-window']).toBe('2px')
  })
})

describe('pure black', () => {
  it('stays readable on black', () => {
    const black = pureBlack(darkroom.colors.dark)
    expect(readabilityProblems(black)).toEqual([])
    expect(black.bg).toBe('#000000')
  })

  it('only changes the dark mode', () => {
    expect(themeToCssVars(darkroom, 'light', true)).toEqual(themeToCssVars(darkroom, 'light'))
    expect(themeToCssVars(darkroom, 'dark', true)['--color-bg']).toBe('#000000')
    expect(themeToCssVars(darkroom, 'dark')['--color-bg']).not.toBe('#000000')
  })

  it('keeps surfaces apart from the background, in order', () => {
    const black = pureBlack(darkroom.colors.dark)
    const lift = (color: string) => contrastRatio(color, '#000000')
    expect(lift(black.surface)).toBeGreaterThan(1)
    expect(lift(black.surface2)).toBeGreaterThan(lift(black.surface))
  })
})

describe('the light palette', () => {
  const palette = darkroom.colors.light
  const ratio = (fg: PaletteKey, bg: PaletteKey) => contrastRatio(palette[fg], palette[bg])
  // the note sits on surface, the sidebar and list on bg, hover, tags and
  // quotes on surface2, the open note's row on selection
  const surfaces: PaletteKey[] = ['bg', 'surface', 'surface2', 'selection']

  it('keeps text and buttons easy to read, it is an accessibility choice', () => {
    for (const surface of surfaces) {
      expect(ratio('text', surface)).toBeGreaterThanOrEqual(12)
      expect(ratio('textMuted', surface)).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    }
    expect(ratio('accentText', 'surface')).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    expect(ratio('accentText', 'surface2')).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    for (const color of ['danger', 'success', 'warning'] as const) {
      expect(ratio(color, 'bg')).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
      expect(ratio(color, 'surface')).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    }
    expect(ratio('accentOn', 'accent')).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    expect(ratio('accentOn', 'accentPressed')).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    expect(ratio('selection', 'bg')).toBeGreaterThan(1.05)
  })
})
