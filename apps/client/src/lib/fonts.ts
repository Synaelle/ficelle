// SPDX-License-Identifier: AGPL-3.0-or-later
// Fonts for notes and code, chosen over the theme's (commands/fonts.rs). A
// font from the list is downloaded once when chosen and loaded from the
// device afterwards; a font installed on the device is used as it is. The
// OpenDyslexic setting still wins (lib/dyslexicFont.ts). The Carbon and
// Classic themes' fonts and OpenDyslexic come the same way, as font packs:
// the app does not carry them, to stay small.

import { backend } from './backend'

export type FontRole = 'notes' | 'code'

/** A chosen font: one from the list (downloaded) or one on the device. */
export type FontChoice = { source: 'list'; id: string } | { source: 'device'; family: string }

export type FontKind = 'sans' | 'serif' | 'mono' | 'readable'

export interface ListedFont {
  /** Fontsource id, also the folder it is kept in. */
  id: string
  family: string
  kind: FontKind
}

/** Free fonts offered for download, all with regular and bold files. */
export const FONT_LIST: ListedFont[] = [
  { id: 'inter', family: 'Inter', kind: 'sans' },
  { id: 'source-sans-3', family: 'Source Sans 3', kind: 'sans' },
  { id: 'ibm-plex-sans', family: 'IBM Plex Sans', kind: 'sans' },
  { id: 'noto-sans', family: 'Noto Sans', kind: 'sans' },
  { id: 'open-sans', family: 'Open Sans', kind: 'sans' },
  { id: 'lato', family: 'Lato', kind: 'sans' },
  { id: 'nunito-sans', family: 'Nunito Sans', kind: 'sans' },
  { id: 'work-sans', family: 'Work Sans', kind: 'sans' },
  { id: 'figtree', family: 'Figtree', kind: 'sans' },
  { id: 'merriweather', family: 'Merriweather', kind: 'serif' },
  { id: 'lora', family: 'Lora', kind: 'serif' },
  { id: 'source-serif-4', family: 'Source Serif 4', kind: 'serif' },
  { id: 'noto-serif', family: 'Noto Serif', kind: 'serif' },
  { id: 'eb-garamond', family: 'EB Garamond', kind: 'serif' },
  { id: 'crimson-pro', family: 'Crimson Pro', kind: 'serif' },
  { id: 'libre-baskerville', family: 'Libre Baskerville', kind: 'serif' },
  { id: 'newsreader', family: 'Newsreader', kind: 'serif' },
  { id: 'lexend', family: 'Lexend', kind: 'readable' },
  { id: 'andika', family: 'Andika', kind: 'readable' },
  { id: 'atkinson-hyperlegible-mono', family: 'Atkinson Hyperlegible Mono', kind: 'readable' },
  { id: 'jetbrains-mono', family: 'JetBrains Mono', kind: 'mono' },
  { id: 'fira-code', family: 'Fira Code', kind: 'mono' },
  { id: 'source-code-pro', family: 'Source Code Pro', kind: 'mono' },
  { id: 'cascadia-code', family: 'Cascadia Code', kind: 'mono' },
  { id: 'roboto-mono', family: 'Roboto Mono', kind: 'mono' },
  { id: 'ubuntu-mono', family: 'Ubuntu Mono', kind: 'mono' },
  { id: 'inconsolata', family: 'Inconsolata', kind: 'mono' },
  { id: 'victor-mono', family: 'Victor Mono', kind: 'mono' },
  { id: 'space-mono', family: 'Space Mono', kind: 'mono' },
  { id: 'commit-mono', family: 'Commit Mono', kind: 'mono' },
]

/** Fontsource's character ranges for its latin and latin-ext files. */
const RANGES = {
  latin:
    'U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD',
  'latin-ext':
    'U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF',
}

export function listedFont(id: string): ListedFont | undefined {
  return FONT_LIST.find((font) => font.id === id)
}

/** The family name a choice puts in css, null for an unknown list id. */
export function familyOf(choice: FontChoice): string | null {
  return choice.source === 'device' ? choice.family : (listedFont(choice.id)?.family ?? null)
}

/** A family name inside double quotes, as css reads it. */
export function quoted(family: string): string {
  return `"${family.replace(/["\\]/g, (c) => `\\${c}`)}"`
}

/** "latin-ext-700-italic" → its subset, weight and style. A variable file,
 * "latin-wght-normal", covers `weights`. */
export function describeFile(name: string, weights = '100 900') {
  const match = /^(latin(?:-ext)?)-(\d{3}|wght)-(normal|italic)$/.exec(name)
  if (!match) return null
  const [, subset, weight, style] = match as unknown as [
    string,
    'latin' | 'latin-ext',
    string,
    string,
  ]
  return { range: RANGES[subset], weight: weight === 'wght' ? weights : weight, style }
}

/** A font fetched like a listed one, with its own files. */
export interface FontPack {
  /** Fontsource id, `:vf` after it for a variable font. */
  id: string
  family: string
  /** Its files, the first one needed, the others used when they come. */
  files: string[]
  /** The weights a variable font's files cover. */
  weights?: string
}

const both = (...names: string[]) => names.flatMap((n) => [`latin-${n}`, `latin-ext-${n}`])

/** The built-in themes' fonts the app does not carry: styles/fonts.css has
 * the default theme's, the web pages carry these in fonts-themes.css. */
export const THEME_FONTS: FontPack[] = [
  {
    id: 'schibsted-grotesk:vf',
    family: 'Schibsted Grotesk',
    files: both('wght-normal'),
    weights: '400 900',
  },
  {
    id: 'literata:vf',
    family: 'Literata',
    files: both('wght-normal', 'wght-italic'),
    weights: '200 900',
  },
  { id: 'ubuntu', family: 'Ubuntu', files: both('400-normal', '500-normal', '700-normal') },
  { id: 'ubuntu-mono', family: 'Ubuntu Mono', files: both('400-normal') },
]

/** The pack for a theme font of that family, if the app downloads it. */
export function themeFontPack(family: string): FontPack | undefined {
  return THEME_FONTS.find((pack) => pack.family === family)
}

/** Hands a font's files to the page as `family`. The first must load, a
 * file missing or broken after it leaves the others. */
async function addFaces(id: string, family: string, names: string[], weights?: string) {
  await Promise.all(
    names.map(async (name, i) => {
      const file = describeFile(name, weights)
      if (!file) return
      try {
        const face = new FontFace(family, await backend.fontFile(id, name), {
          weight: file.weight,
          style: file.style,
          unicodeRange: file.range,
          display: 'swap',
        })
        document.fonts.add(await face.load())
      } catch (err) {
        if (i === 0) throw err
      }
    }),
  )
}

const packs = new Map<string, Promise<boolean>>()

/** Makes a pack usable on the page as `family`: downloaded when this device
 * does not have it yet (`download` false only reads what is there). Each
 * pack is loaded once a page; one that did not come is tried again on the
 * next call. */
export function loadFontPack(
  pack: FontPack,
  download: boolean,
  family = pack.family,
): Promise<boolean> {
  const key = `${pack.id} ${family}`
  const known = packs.get(key)
  if (known) return known
  const loading = (async () => {
    let files = await backend.fontFiles(pack.id, pack.files)
    if (!files.includes(pack.files[0] ?? '')) {
      if (!download) return false
      files = await backend.fontDownload(pack.id, pack.files)
    }
    // the list's order, the needed file first
    await addFaces(
      pack.id,
      family,
      pack.files.filter((name) => files.includes(name)),
      pack.weights,
    )
    return true
  })()
  packs.set(key, loading)
  loading.then(
    (ok) => ok || packs.delete(key),
    () => packs.delete(key),
  )
  return loading
}

const loaded = new Set<string>()

/** Makes a listed font usable on the page: downloads it when this device
 * does not have it yet (`download` false only reads what is there), then
 * hands its files to the page. */
export async function loadListedFont(id: string, download: boolean): Promise<boolean> {
  if (loaded.has(id)) return true
  const font = listedFont(id)
  if (!font) return false
  let files = await backend.fontFiles(id)
  if (!files.includes('latin-400-normal')) {
    if (!download) return false
    files = await backend.fontDownload(id)
  }
  await Promise.all(
    files.map(async (name) => {
      const file = describeFile(name)
      if (!file) return
      const face = new FontFace(font.family, await backend.fontFile(id, name), {
        weight: file.weight,
        style: file.style,
        unicodeRange: file.range,
        display: 'swap',
      })
      document.fonts.add(await face.load())
    }),
  )
  loaded.add(id)
  return true
}
