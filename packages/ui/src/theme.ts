// SPDX-License-Identifier: AGPL-3.0-or-later
// The app's theme model. The json file in ../themes holds the palettes and
// sizes, this module checks that they read well and turns them into css
// variables.

import { contrastRatio, mix } from './color'

export const PALETTE_KEYS = [
  'bg',
  'surface',
  'surface2',
  'border',
  'borderStrong',
  'text',
  'textMuted',
  'textFaint',
  'accent',
  'accentText',
  'accentPressed',
  'accentOn',
  'danger',
  'success',
  'warning',
  // callouts: a note and an important one, next to success, warning and danger
  'info',
  'important',
  'selection',
] as const

export type PaletteKey = (typeof PALETTE_KEYS)[number]
export type Palette = Record<PaletteKey, string>
export type Mode = 'dark' | 'light'

export interface Theme {
  id: string
  name: string
  description: string
  fonts: { ui: string; editor: string; mono: string }
  icons: 'lucide' | 'phosphor-regular' | 'stitch'
  colors: Record<Mode, Palette>
  /** One small corner everywhere, so nothing round sits beside something square. */
  radius: { control: number; popover: number; window: number }
  space: number[]
  motion: { fast: number; base: number; slow: number; easeOut: string; easeInOut: string }
}

/** Minimum contrast for anything meant to be read (WCAG AA, normal text). */
export const MIN_TEXT_CONTRAST = 4.5

/** Pairs that people read, each must reach MIN_TEXT_CONTRAST. */
const READ_PAIRS: [PaletteKey, PaletteKey][] = [
  ['text', 'bg'],
  ['text', 'surface'],
  ['textMuted', 'bg'],
  ['accentText', 'bg'],
  // links and accent words inside raised panels and code
  ['accentText', 'surface2'],
  ['accentOn', 'accent'],
  // a primary button under the pointer
  ['accentOn', 'accentPressed'],
  // danger is read as text on the background and as a fill under it
  ['danger', 'bg'],
  // callout titles, written in their colour
  ['info', 'bg'],
  ['important', 'bg'],
  // the selected note row keeps its muted snippet and date readable
  ['textMuted', 'selection'],
]

/** Minimum contrast for the edge that shows where a control is (WCAG AA). */
export const MIN_BOUNDARY_CONTRAST = 3

/** Edges of fields, each must reach MIN_BOUNDARY_CONTRAST. */
const BOUNDARY_PAIRS: [PaletteKey, PaletteKey][] = [
  ['borderStrong', 'bg'],
  ['borderStrong', 'surface'],
]

/** What in a palette is too hard to read, empty when all of it reads well. */
export function readabilityProblems(palette: Palette): string[] {
  const check = (pairs: [PaletteKey, PaletteKey][], min: number) =>
    pairs.flatMap(([fg, bg]) => {
      const ratio = contrastRatio(palette[fg], palette[bg])
      return ratio < min ? [`${fg} on ${bg} is ${ratio.toFixed(2)}:1, needs ${min}:1`] : []
    })
  return [...check(READ_PAIRS, MIN_TEXT_CONTRAST), ...check(BOUNDARY_PAIRS, MIN_BOUNDARY_CONTRAST)]
}

/** A dark palette on pure black, for OLED screens: the background turns
 * black and the surfaces and borders come closer to it, in the same order,
 * so panels still stand apart. Text only gains contrast. */
export function pureBlack(palette: Palette): Palette {
  const black = '#000000'
  return {
    ...palette,
    bg: black,
    surface: mix(black, palette.surface, 0.55),
    surface2: mix(black, palette.surface2, 0.7),
    border: mix(black, palette.border, 0.85),
    borderStrong: mix(black, palette.borderStrong, 0.95),
    selection: mix(black, palette.selection, 0.85),
  }
}

const kebab = (key: string) => key.replace(/[A-Z0-9]/g, (c) => `-${c.toLowerCase()}`)

/** Css custom properties for the theme and a mode, ready to set on the root
 * element. `black` turns the dark mode pure black, light stays as it is. */
export function themeToCssVars(theme: Theme, mode: Mode, black = false): Record<string, string> {
  const palette = mode === 'dark' && black ? pureBlack(theme.colors.dark) : theme.colors[mode]
  const vars: Record<string, string> = {
    '--font-ui': `"${theme.fonts.ui}", system-ui, sans-serif`,
    '--font-editor': `"${theme.fonts.editor}", system-ui, sans-serif`,
    '--font-mono': `"${theme.fonts.mono}", ui-monospace, monospace`,
    '--radius-control': `${theme.radius.control}px`,
    '--radius-popover': `${theme.radius.popover}px`,
    '--radius-window': `${theme.radius.window}px`,
    '--dur-fast': `${theme.motion.fast}ms`,
    '--dur-base': `${theme.motion.base}ms`,
    '--dur-slow': `${theme.motion.slow}ms`,
    '--ease-out': theme.motion.easeOut,
    '--ease-in-out': theme.motion.easeInOut,
  }
  for (const key of PALETTE_KEYS) vars[`--color-${kebab(key)}`] = palette[key]
  theme.space.forEach((size, i) => (vars[`--space-${i + 1}`] = `${size}px`))
  return vars
}
