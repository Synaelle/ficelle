// SPDX-License-Identifier: AGPL-3.0-or-later
// Theme model shared by built-in and user themes. The json files in
// ../themes follow theme.schema.json, this module validates them at runtime
// (user themes come from imports) and turns them into css variables.

import { contrastRatio, isHexColor, mix, pickReadable, readableOn } from './color'

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
  'selection',
] as const

export type PaletteKey = (typeof PALETTE_KEYS)[number]
export type Palette = Record<PaletteKey, string>
export type Mode = 'dark' | 'light'
export type ModePreference = 'system' | Mode

export interface Theme {
  id: string
  name: string
  description: string
  fonts: { ui: string; editor: string; mono: string }
  icons: 'lucide' | 'phosphor-regular' | 'stitch'
  colors: Record<Mode, Palette>
  radius: { control: number; popover: number; window: number }
  space: number[]
  motion: { fast: number; base: number; slow: number; easeOut: string; easeInOut: string }
  /** 2 for themes that can carry css, absent in the first themes. */
  version?: number
  /** Extra css, applied after the tokens, see themeCssProblems. */
  css?: string
}

/** Css a theme may carry, fonts as data included. */
export const MAX_THEME_CSS = 1024 * 1024

/** Minimum contrast for anything meant to be read (WCAG AA, normal text). */
export const MIN_TEXT_CONTRAST = 4.5

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const isSize = (value: unknown, max: number) =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= max

// sizes beyond these only break the layout
const MAX_RADIUS = 64
const MAX_SPACE = 128
const MAX_DURATION = 2000

// a font name goes inside quotes in a css value: nothing that could end them
const isFontName = (value: unknown) =>
  typeof value === 'string' && /^[\p{L}\p{N} ._-]{1,64}$/u.test(value)

/**
 * Checks shape, color format and readability. Returns readable problems,
 * an empty list means the theme can be used.
 */
export function validateTheme(input: unknown): string[] {
  const problems: string[] = []
  if (!isObject(input)) return ['theme must be an object']

  if (typeof input.id !== 'string' || !/^[a-z0-9-]{1,40}$/.test(input.id)) {
    problems.push('id must be 1 to 40 lowercase letters, digits or dashes')
  }
  if (typeof input.name !== 'string' || input.name.trim() === '' || input.name.length > 60) {
    problems.push('name must be text of 1 to 60 characters')
  }
  if (typeof input.description !== 'string' || input.description.length > 200) {
    problems.push('description must be text of at most 200 characters')
  }

  const fonts = input.fonts
  if (!isObject(fonts) || ['ui', 'editor', 'mono'].some((k) => !isFontName(fonts[k]))) {
    problems.push('fonts needs ui, editor and mono, each a font name')
  }
  if (!['lucide', 'phosphor-regular', 'stitch'].includes(input.icons as string)) {
    problems.push('icons must be lucide, phosphor-regular or stitch')
  }

  const colors = input.colors
  for (const mode of ['dark', 'light'] as const) {
    const palette = isObject(colors) ? colors[mode] : undefined
    if (!isObject(palette)) {
      problems.push(`colors.${mode} is missing`)
      continue
    }
    for (const key of PALETTE_KEYS) {
      if (!isHexColor(palette[key])) problems.push(`colors.${mode}.${key} must be a #rrggbb color`)
    }
    if (PALETTE_KEYS.every((key) => isHexColor(palette[key]))) {
      problems.push(...readabilityProblems(palette as Palette, mode))
    }
  }

  const radius = input.radius
  if (
    !isObject(radius) ||
    ['control', 'popover', 'window'].some((k) => !isSize(radius[k], MAX_RADIUS))
  ) {
    problems.push(`radius needs control, popover and window sizes, at most ${MAX_RADIUS}`)
  }
  if (
    !Array.isArray(input.space) ||
    input.space.length !== 7 ||
    !input.space.every((size) => isSize(size, MAX_SPACE))
  ) {
    problems.push(`space must list 7 sizes, at most ${MAX_SPACE}`)
  }

  const motion = input.motion
  if (
    !isObject(motion) ||
    ['fast', 'base', 'slow'].some((k) => !isSize(motion[k], MAX_DURATION)) ||
    ['easeOut', 'easeInOut'].some(
      (k) => !/^cubic-bezier\(\s*[-\d.]+(\s*,\s*[-\d.]+){3}\s*\)$/.test(String(motion[k])),
    )
  ) {
    problems.push('motion needs fast, base, slow durations and two cubic-bezier easings')
  }

  if (input.version !== undefined && input.version !== 1 && input.version !== 2) {
    problems.push('version must be 1 or 2')
  }
  if (input.css !== undefined) {
    if (typeof input.css !== 'string') problems.push('css must be text')
    else problems.push(...themeCssProblems(input.css))
  }

  return problems
}

/**
 * What a theme's css may not do: load anything from outside the theme
 * (the app's content policy refuses it too, this names the rule), or use
 * escapes, which could spell those in a way a plain reading would miss.
 * Fonts and pictures come as data: addresses.
 */
export function themeCssProblems(css: string): string[] {
  if (css.length > MAX_THEME_CSS) return [`css must be at most ${MAX_THEME_CSS / 1024} KB`]
  const problems: string[] = []
  // read as written: comments cannot split a token (`@im/**/port` is not an
  // import), and removing them first would let a "/*" inside a string hide
  // the rule after it
  const plain = css
  if (plain.includes('\\')) problems.push('css may not use backslash escapes')
  if (/@import/i.test(plain)) problems.push('css may not use @import')
  if (/image-set\s*\(|(^|[^-\w])src\s*\(/i.test(plain)) {
    problems.push('css may not use image-set() or src(), use url(data:...)')
  }
  for (const match of plain.matchAll(/url\s*\(\s*(['"]?)([^'")\s]*)/gi)) {
    if (!/^data:/i.test(match[2] ?? '')) {
      problems.push(`css may only load data: addresses, not url(${match[1]}${match[2]})`)
    }
  }
  return problems
}

/** Pairs that people read, each must reach MIN_TEXT_CONTRAST. */
const READ_PAIRS: [PaletteKey, PaletteKey][] = [
  ['text', 'bg'],
  ['text', 'surface'],
  ['textMuted', 'bg'],
  ['accentText', 'bg'],
  ['accentOn', 'accent'],
  // danger is read as text on the background and as a fill under it
  ['danger', 'bg'],
  // the selected note row keeps its muted snippet and date readable
  ['textMuted', 'selection'],
]

function readabilityProblems(palette: Palette, mode: Mode): string[] {
  return READ_PAIRS.flatMap(([fg, bg]) => {
    const ratio = contrastRatio(palette[fg], palette[bg])
    return ratio < MIN_TEXT_CONTRAST
      ? [`colors.${mode}: ${fg} on ${bg} is ${ratio.toFixed(2)}:1, needs ${MIN_TEXT_CONTRAST}:1`]
      : []
  })
}

/**
 * Picks the mode to render. Dark is the fallback when the platform reports no
 * preference, as decided for the product.
 */
export function resolveMode(preference: ModePreference, systemPrefersLight: boolean | null): Mode {
  if (preference !== 'system') return preference
  return systemPrefersLight === true ? 'light' : 'dark'
}

/** How much of a custom accent tints the selection at most. */
const SELECTION_TINT = 0.2
/** How far the pressed state moves from a custom accent toward the background. */
const PRESSED_SHADE = 0.2

/**
 * Palette with an optional user accent applied. Everything the theme derives
 * from its accent follows it too (selection tint, pressed state, link text),
 * links get nudged toward the body text until they are readable.
 */
export function withAccent(palette: Palette, accent?: string): Palette {
  if (!accent || !isHexColor(accent)) return palette
  return {
    ...palette,
    accent,
    accentPressed: mix(accent, palette.bg, PRESSED_SHADE),
    accentText: readableOn(accent, palette.bg, palette.text, MIN_TEXT_CONTRAST),
    accentOn: pickReadable(accent, [palette.bg, palette.text]),
    selection: readableSelection(palette, accent),
  }
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
    borderStrong: mix(black, palette.borderStrong, 0.9),
    selection: mix(black, palette.selection, 0.85),
  }
}

/** The strongest accent tint, up to SELECTION_TINT, that muted text still reads on. */
function readableSelection(palette: Palette, accent: string): string {
  for (let tint = SELECTION_TINT; tint > 0; tint -= 0.02) {
    const selection = mix(palette.bg, accent, tint)
    if (contrastRatio(palette.textMuted, selection) >= MIN_TEXT_CONTRAST) return selection
  }
  return palette.bg
}

const kebab = (key: string) => key.replace(/[A-Z0-9]/g, (c) => `-${c.toLowerCase()}`)

/** Css custom properties for one theme and mode, ready to set on the root
 * element. `black` turns the dark mode pure black, the light one stays. */
export function themeToCssVars(
  theme: Theme,
  mode: Mode,
  accent?: string,
  black = false,
): Record<string, string> {
  const base = theme.colors[mode]
  const palette = withAccent(mode === 'dark' && black ? pureBlack(base) : base, accent)
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
