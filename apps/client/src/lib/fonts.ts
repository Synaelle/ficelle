// SPDX-License-Identifier: AGPL-3.0-or-later
// A font the app downloads once and keeps on the device (commands/fonts.rs):
// only OpenDyslexic (lib/dyslexicFont.ts), the app does not carry it, to stay
// small.

import { backend } from './backend'

/** Every arrow, which the app's fonts lack (styles/arrows.css, made by
 * tools/arrows-font.py): right behind the first font of a stack. */
export const ARROWS_FONT = "'Ficelle Arrows'"

/** Fontsource's character ranges for its latin and latin-ext files. */
const RANGES = {
  latin:
    'U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD',
  'latin-ext':
    'U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF',
}

/** "latin-ext-700-italic" → its subset, weight and style. */
export function describeFile(name: string) {
  const match = /^(latin(?:-ext)?)-(\d{3})-(normal|italic)$/.exec(name)
  if (!match) return null
  const [, subset, weight, style] = match as unknown as [
    string,
    'latin' | 'latin-ext',
    string,
    string,
  ]
  return { range: RANGES[subset], weight, style }
}

/** A font fetched like a listed one, with its own files. */
export interface FontPack {
  /** Fontsource id. */
  id: string
  family: string
  /** Its files, the first one needed, the others used when they come. */
  files: string[]
}

/** Hands a font's files to the page as `family`. The first must load, a
 * file missing or broken after it leaves the others. */
async function addFaces(id: string, family: string, names: string[]) {
  await Promise.all(
    names.map(async (name, i) => {
      const file = describeFile(name)
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
