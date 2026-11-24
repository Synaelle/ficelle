// SPDX-License-Identifier: AGPL-3.0-or-later
// The text and code fonts picked in Settings, Editor. The defaults come with
// the app (styles/fonts.css). Ubuntu and JetBrains Mono are downloaded the
// first time they are picked (lib/fonts.ts) and kept on the device, the app
// does not carry them, to stay small. Until one is here, and when it cannot
// be had, the page keeps the default.

import { shownError, type ShownError } from './errors'
import { type FontPack, loadFontPack } from './fonts'
import { prefs, type CodeFont, type TextFont } from './prefs.svelte'

type Downloaded = Exclude<TextFont | CodeFont, 'default'>

/** Latin and its extension, regular, bold and their italics. */
const FILES = [
  'latin-400-normal',
  'latin-700-normal',
  'latin-400-italic',
  'latin-700-italic',
  'latin-ext-400-normal',
  'latin-ext-700-normal',
  'latin-ext-400-italic',
  'latin-ext-700-italic',
]

/** The fonts downloaded when picked, from Fontsource. */
export const FONT_PACKS: Record<Downloaded, FontPack> = {
  ubuntu: { id: 'ubuntu', family: 'Ubuntu', files: FILES },
  jetbrainsMono: { id: 'jetbrains-mono', family: 'JetBrains Mono', files: FILES },
}

/** The bundled code font, named the same in every language. */
export const DEFAULT_CODE_FONT = 'Ubuntu Mono'

export type FontKind = 'text' | 'code'

interface FontLoad {
  /** The family the page uses, null for the default. */
  family: string | null
  /** The picked font is being downloaded, the default shows meanwhile. */
  loading: boolean
  /** Why the picked font did not come. */
  error: ShownError | null
}

/** Where each pick stands, for the theme and the settings pages. */
export const fontStatus: Record<FontKind, FontLoad> = $state({
  text: { family: null, loading: false, error: null },
  code: { family: null, loading: false, error: null },
})

/** The latest call of each kind: a slow download after the pick changed
 * again does nothing. */
const latest: Record<FontKind, number> = { text: 0, code: 0 }

/** Uses `font` for `kind`, downloading it first when this device lacks it. */
export function applyFont(kind: FontKind, font: TextFont | CodeFont) {
  const call = ++latest[kind]
  const status = fontStatus[kind]
  status.error = null
  if (font === 'default') {
    status.family = null
    status.loading = false
    return
  }
  const pack = FONT_PACKS[font]
  status.loading = true
  loadFontPack(pack, true).then(
    () => {
      if (call !== latest[kind]) return
      status.family = pack.family
      status.loading = false
    },
    (err: unknown) => {
      if (call !== latest[kind]) return
      status.family = null
      status.loading = false
      status.error = shownError(err, 'FONT')
    },
  )
}

/** Keeps the page on the picked fonts. Call once from each root component. */
export function applyChosenFonts() {
  $effect(() => applyFont('text', prefs.font))
  $effect(() => applyFont('code', prefs.codeFont))
}
