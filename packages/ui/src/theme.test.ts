// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import { builtInThemes, DEFAULT_THEME_ID } from '../themes'
import darkroomJson from '../themes/darkroom.json' with { type: 'json' }
import { contrastRatio, luminance } from './color'
import {
  cornerRadius,
  MAX_THEME_CSS,
  MIN_TEXT_CONTRAST,
  modePalette,
  pureBlack,
  ROUND_RADIUS,
  resolveMode,
  themeCssProblems,
  themeToCssVars,
  validateTheme,
  withAccent,
  type Palette,
  type PaletteKey,
  type Theme,
} from './theme'

const darkroom = darkroomJson as Theme

describe('built-in themes', () => {
  it.each(builtInThemes.map((t) => [t.id, t] as const))('%s is valid and readable', (_, theme) => {
    expect(validateTheme(theme)).toEqual([])
  })

  it('have unique ids and include the default', () => {
    const ids = builtInThemes.map((t) => t.id)
    expect(new Set(ids).size).toBe(ids.length)
    expect(ids).toContain(DEFAULT_THEME_ID)
  })
})

describe('validateTheme', () => {
  const base = darkroom

  it('reports a color that is not hex', () => {
    const broken = structuredClone(base)
    broken.colors.dark.text = 'red'
    expect(validateTheme(broken)).toContain('colors.dark.text must be a #rrggbb color')
  })

  it('reports unreadable text', () => {
    const broken = structuredClone(base)
    broken.colors.light.text = broken.colors.light.bg
    expect(validateTheme(broken).some((p) => p.startsWith('colors.light: text on bg'))).toBe(true)
  })

  it('rejects anything that is not an object', () => {
    expect(validateTheme(null)).toEqual(['theme must be an object'])
  })

  it('refuses font names that could end their quotes', () => {
    for (const name of ['A"; } body { display: none', 'A\\B', 'A{', '']) {
      const broken = structuredClone(base)
      broken.fonts.ui = name
      expect(validateTheme(broken)).toContain('fonts needs ui, editor and mono, each a font name')
    }
    const accented = structuredClone(base)
    accented.fonts.ui = 'Éclat Sans 2'
    expect(validateTheme(accented)).toEqual([])
  })

  it('refuses sizes and timings that only break the layout', () => {
    const broken = structuredClone(base)
    broken.radius.window = 500
    broken.space[6] = Infinity
    broken.motion.slow = 60_000
    broken.motion.easeOut = 'cubic-bezier(1, 2, 3, 4); color: red'
    const problems = validateTheme(broken)
    expect(problems.some((p) => p.startsWith('radius'))).toBe(true)
    expect(problems.some((p) => p.startsWith('space'))).toBe(true)
    expect(problems.some((p) => p.startsWith('motion'))).toBe(true)
  })

  it('takes a version 2 theme with css', () => {
    const v2 = {
      ...structuredClone(base),
      id: 'mine',
      version: 2,
      css: '/* a comment */ :root[data-mode="dark"] .editor { letter-spacing: 0.01em }\n@font-face { font-family: "Mine"; src: url(data:font/woff2;base64,d09GMg) }',
    }
    expect(validateTheme(v2)).toEqual([])
    expect(validateTheme({ ...v2, version: 3 })).toContain('version must be 1 or 2')
    expect(validateTheme({ ...v2, css: 4 })).toContain('css must be text')
  })
})

describe('themeCssProblems', () => {
  it.each([
    ['@import "https://example.com/x.css";', '@import'],
    ['@IMPORT url(x.css);', '@import'],
    ['.a { background: url(https://example.com/x.png) }', 'data: addresses'],
    ['.a { background: URL( "//example.com/x.png" ) }', 'data: addresses'],
    ['.a { background: url(x.png) }', 'data: addresses'],
    ['.a { background: u\\72l(https://example.com) }', 'escapes'],
    ['.a { background: image-set("https://example.com/x.png" 1x) }', 'image-set'],
    ['@font-face { src: src("https://example.com/f.woff2") }', 'image-set'],
    // a "/*" inside a string must not hide what follows from the check
    ['.a { content: "/*" } .b { background: url(https://example.com) } /* */', 'data: addresses'],
    // every property that loads something, each way of writing the address
    ...[
      'background-image',
      'border-image',
      'list-style',
      'cursor',
      'content',
      'mask-image',
      '-webkit-mask-box-image',
      'shape-outside',
      'filter',
      'clip-path',
      'offset-path',
      'marker',
    ].map((p): [string, string] => [
      `.a { ${p}: url(https://example.com/${p}) }`,
      'data: addresses',
    ]),
    ['.a { background: url(\n  "https://example.com" ) }', 'data: addresses'],
    ["@font-face { font-family: x; src: url('https://example.com/f.woff2') }", 'data: addresses'],
    ['.a { background: cross-fade(url(https://example.com) 50%, red) }', 'data: addresses'],
    ['.a { background: -webkit-image-set("https://example.com/x.png" 1x) }', 'image-set'],
    ['.a { background: url(data:x), url(https://example.com) }', 'data: addresses'],
    ['.a { background: url( data:x ) } .b { background: url(//example.com) }', 'data: addresses'],
    ['@counter-style s { system: cyclic; symbols: url(https://example.com) }', 'data: addresses'],
    ['@import"https://example.com/x.css";', '@import'],
    ['.a { background: url(javascript:alert(1)) }', 'data: addresses'],
    ['.a { background: url(attachment://x) }', 'data: addresses'],
  ])('refuses %s', (css, problem) => {
    expect(themeCssProblems(css).join(' ')).toContain(problem)
  })

  it('takes data: addresses and plain rules', () => {
    expect(
      themeCssProblems(
        '.a { background: url("data:image/png;base64,AAAA"); color: var(--color-text) }',
      ),
    ).toEqual([])
  })

  it('refuses css that is too large', () => {
    expect(themeCssProblems('a'.repeat(MAX_THEME_CSS + 1))).toHaveLength(1)
  })
})

describe('resolveMode', () => {
  it('follows the system when asked', () => {
    expect(resolveMode('system', true)).toBe('light')
    expect(resolveMode('system', false)).toBe('dark')
  })

  it('falls back to dark when the system has no preference', () => {
    expect(resolveMode('system', null)).toBe('dark')
  })

  it('lets an explicit choice win', () => {
    expect(resolveMode('light', false)).toBe('light')
    expect(resolveMode('dark', true)).toBe('dark')
  })
})

describe('withAccent', () => {
  const palette = darkroom.colors.dark

  it('puts the more readable of bg and text on a custom accent', () => {
    for (const accent of ['#E6DFD8', '#4F9BD9', '#6FA45A', '#1A1A1A']) {
      const best = Math.max(contrastRatio(accent, palette.bg), contrastRatio(accent, palette.text))
      expect(contrastRatio(withAccent(palette, accent).accentOn, accent)).toBe(best)
    }
  })

  it('keeps link text and selected rows readable for any accent', () => {
    for (const theme of builtInThemes) {
      for (const mode of ['dark', 'light'] as const) {
        const base = theme.colors[mode]
        for (const accent of ['#2A0A0A', '#4F9BD9', '#FFE600', '#6FA45A', '#E0567A']) {
          const p = withAccent(base, accent)
          expect(contrastRatio(p.accentText, p.bg)).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
          expect(contrastRatio(p.text, p.selection)).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
          expect(contrastRatio(p.textMuted, p.selection)).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
        }
      }
    }
  })

  it('tints the selection and pressed state with the accent', () => {
    const p = withAccent(palette, '#4F9BD9')
    expect(p.selection).not.toBe(palette.selection)
    expect(p.accentPressed).not.toBe(p.accent)
  })

  it('ignores an invalid accent', () => {
    expect(withAccent(palette, 'nope')).toBe(palette)
  })
})

describe('themeToCssVars', () => {
  it('exposes colors, sizes and motion as css variables', () => {
    const vars = themeToCssVars(darkroom, 'light', '#4F9BD9')
    expect(vars['--color-accent']).toBe('#4F9BD9')
    expect(vars['--color-text-muted']).toBe(darkroom.colors.light.textMuted)
    expect(vars['--color-surface2']).toBeUndefined()
    expect(vars['--color-surface-2']).toBe(darkroom.colors.light.surface2)
    expect(vars['--space-7']).toBe('48px')
    expect(vars['--dur-fast']).toBe('140ms')
  })
})

describe('corners', () => {
  it('has one small corner everywhere when square', () => {
    const vars = themeToCssVars(darkroom, 'dark')
    const corner = `${darkroom.radius.control}px`
    expect(vars['--radius-control']).toBe(corner)
    expect(vars['--radius-popover']).toBe(corner)
    expect(vars['--radius-window']).toBe(corner)
    expect(themeToCssVars(darkroom, 'dark', undefined, false, 'square')).toEqual(vars)
  })

  it('rounds a square theme', () => {
    const vars = themeToCssVars(darkroom, 'dark', undefined, false, 'round')
    expect(vars['--radius-control']).toBe(`${ROUND_RADIUS.control}px`)
    expect(vars['--radius-popover']).toBe(`${ROUND_RADIUS.popover}px`)
    expect(vars['--radius-window']).toBe(`${ROUND_RADIUS.window}px`)
  })

  it('leaves a rounder theme its own corners', () => {
    expect(cornerRadius({ control: 16, popover: 8, window: 30 }, 'round')).toEqual({
      control: 16,
      popover: ROUND_RADIUS.popover,
      window: 30,
    })
  })

  it('nests round corners inside each other', () => {
    expect(ROUND_RADIUS.control).toBeLessThan(ROUND_RADIUS.popover)
    expect(ROUND_RADIUS.popover).toBeLessThan(ROUND_RADIUS.window)
  })
})

describe('pure black', () => {
  it.each(builtInThemes.map((t) => [t.id, t] as const))(
    '%s stays readable on black',
    (_, theme) => {
      const black = { ...theme, colors: { ...theme.colors, dark: pureBlack(theme.colors.dark) } }
      expect(validateTheme(black)).toEqual([])
      expect(black.colors.dark.bg).toBe('#000000')
    },
  )

  it('only changes the dark mode', () => {
    expect(themeToCssVars(darkroom, 'light', undefined, true)).toEqual(
      themeToCssVars(darkroom, 'light'),
    )
    expect(themeToCssVars(darkroom, 'dark', undefined, true)['--color-bg']).toBe('#000000')
    expect(themeToCssVars(darkroom, 'dark')['--color-bg']).not.toBe('#000000')
  })

  it('keeps surfaces apart from the background, in order', () => {
    const black = pureBlack(darkroom.colors.dark)
    const lift = (color: string) => contrastRatio(color, '#000000')
    expect(lift(black.surface)).toBeGreaterThan(1)
    expect(lift(black.surface2)).toBeGreaterThan(lift(black.surface))
  })
})

describe('the default light palettes', () => {
  const light = darkroom.colors.light
  // checked to be there below, as a palette for the other checks
  const warmLight = darkroom.colors.warmLight as Palette
  // the note sits on surface, the sidebar and list on bg, hover, tags and
  // quotes on surface2, the open note's row on selection
  const surfaces: PaletteKey[] = ['bg', 'surface', 'surface2', 'selection']

  it.each([
    ['light', light],
    ['warm paper', warmLight],
  ] as const)('%s keeps text and buttons readable', (_, palette) => {
    const ratio = (fg: PaletteKey, bg: PaletteKey) => contrastRatio(palette[fg], palette[bg])
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

  describe('warm paper', () => {
    const palette = warmLight
    const ratio = (fg: PaletteKey, bg: PaletteKey) => contrastRatio(palette[fg], palette[bg])

    it('is offered by the default theme and readable as a theme palette', () => {
      expect(warmLight).toBeDefined()
      expect(validateTheme(darkroom)).toEqual([])
    })

    it.each(surfaces)('reads links on %s too', (surface) => {
      expect(ratio('accentText', surface)).toBeGreaterThanOrEqual(MIN_TEXT_CONTRAST)
    })

    it('keeps hints and field edges visible', () => {
      expect(ratio('textFaint', 'bg')).toBeGreaterThanOrEqual(3)
      expect(ratio('textFaint', 'surface')).toBeGreaterThanOrEqual(3)
      expect(ratio('borderStrong', 'surface')).toBeGreaterThanOrEqual(2.5)
      expect(ratio('border', 'surface')).toBeGreaterThan(1.3)
    })

    it('has no glaring white surface, the note page the lightest', () => {
      for (const surface of surfaces) expect(luminance(palette[surface])).toBeLessThan(0.9)
      expect(luminance(palette.surface)).toBeGreaterThan(luminance(palette.bg))
      expect(luminance(palette.bg)).toBeGreaterThan(luminance(palette.surface2))
    })
  })

  it('is used only when chosen, and only in light mode', () => {
    expect(modePalette(darkroom, 'light')).toBe(light)
    expect(modePalette(darkroom, 'light', false, true)).toBe(warmLight)
    expect(modePalette(darkroom, 'dark', false, true)).toBe(darkroom.colors.dark)
    expect(themeToCssVars(darkroom, 'dark', undefined, false, 'square', true)).toEqual(
      themeToCssVars(darkroom, 'dark'),
    )
    expect(themeToCssVars(darkroom, 'light', undefined, false, 'square', true)['--color-bg']).toBe(
      warmLight.bg,
    )
  })

  it('falls back to light in a theme without warm paper', () => {
    const plain = structuredClone(darkroom)
    delete plain.colors.warmLight
    expect(validateTheme(plain)).toEqual([])
    expect(modePalette(plain, 'light', false, true)).toEqual(plain.colors.light)
  })

  it('checks a warm palette like the others', () => {
    const broken = structuredClone(darkroom)
    broken.colors.warmLight = { ...warmLight, text: warmLight.bg }
    expect(validateTheme(broken).some((p) => p.startsWith('colors.warmLight: text on bg'))).toBe(
      true,
    )
  })
})
