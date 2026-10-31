// SPDX-License-Identifier: AGPL-3.0-or-later
// Copies every font file the apps may ask for from Fontsource on jsDelivr
// into a folder the server serves at /fonts/ (apps/server/src/web.rs), so
// the apps load fonts from the project's own host and no third party sees
// which font a device uses (commands/fonts.rs asks there first).
//
//   node tools/fonts-mirror.mjs <folder>
//
// The fonts are the ones in apps/client/src/lib/fonts.ts (the list and the
// themes' packs) and OpenDyslexic (lib/dyslexicFont.ts); the files, those
// commands/fonts.rs accepts. A file a font does not have is skipped; a font
// whose regular file cannot be had stops the mirror. All are open fonts
// (SIL Open Font License or similar), free to copy.
//
// The Docker build runs it into the web pages' folder: the image carries
// them, a few megabytes.

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const out = process.argv[2]
if (!out) {
  console.error('usage: node tools/fonts-mirror.mjs <folder>')
  process.exit(1)
}

const CDN = 'https://cdn.jsdelivr.net/fontsource/fonts'
/** As KNOWN in commands/fonts.rs. */
const FILES = [
  'latin-400-normal',
  'latin-700-normal',
  'latin-400-italic',
  'latin-700-italic',
  'latin-ext-400-normal',
  'latin-ext-700-normal',
  'latin-ext-400-italic',
  'latin-ext-700-italic',
  'latin-500-normal',
  'latin-ext-500-normal',
  'latin-wght-normal',
  'latin-wght-italic',
  'latin-ext-wght-normal',
  'latin-ext-wght-italic',
]

const source = readFileSync(join(root, 'apps/client/src/lib/fonts.ts'), 'utf8')
const ids = [...new Set([...source.matchAll(/\bid: '([a-z0-9:-]+)'/g)].map((m) => m[1]))]
ids.push('opendyslexic')
if (ids.length < 30) {
  console.error(`only ${ids.length} fonts found in fonts.ts, the list moved?`)
  process.exit(1)
}

/**
 * One file, `null` when the font has none. Tried three times.
 * @param {string} url
 */
async function fetchFile(url) {
  let last
  for (let i = 0; i < 3; i++) {
    try {
      const response = await fetch(url, { headers: { 'user-agent': 'ficelle' } })
      if (response.status === 404) return null
      if (!response.ok) throw new Error(`${response.status}`)
      const bytes = Buffer.from(await response.arrayBuffer())
      // a woff2 file starts with wOF2
      return bytes.subarray(0, 4).toString() === 'wOF2' ? bytes : null
    } catch (err) {
      last = err
      await new Promise((r) => setTimeout(r, 500))
    }
  }
  throw new Error(`${url}: ${last}`)
}

// a variable font's files only have wght names, the others only weights
const wanted = (/** @type {string} */ id) =>
  FILES.filter((name) => name.includes('-wght-') === id.endsWith(':vf'))

let count = 0
let bytes = 0
for (const id of ids) {
  // the folder a variable font is kept in on the devices too
  const folder = join(out, id.replace(':vf', '@vf'))
  mkdirSync(folder, { recursive: true })
  const names = wanted(id)
  let got = 0
  // a few at a time, jsDelivr is fast
  await Promise.all(
    names.map(async (name) => {
      const file = await fetchFile(`${CDN}/${id}@latest/${name}.woff2`)
      if (!file) return
      writeFileSync(join(folder, `${name}.woff2`), file)
      got += 1
      count += 1
      bytes += file.length
    }),
  )
  if (got === 0) {
    console.error(`${id}: no file at all`)
    process.exit(1)
  }
}
console.log(`${ids.length} fonts, ${count} files, ${(bytes / 1024 / 1024).toFixed(1)} MB in ${out}`)
