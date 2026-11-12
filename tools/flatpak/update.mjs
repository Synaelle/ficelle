// SPDX-License-Identifier: AGPL-3.0-or-later
// Fills the Flathub manifest for a stable release, adds the release to the
// metainfo here, and writes both with the launcher into a folder, ready to
// commit to the Flathub repository (github.com/flathub/com.synaelle.ficelle).
//
//   node tools/flatpak/update.mjs <version> <folder>
//
// The checksum comes from the release's own AppImage and the date from the
// release, both from GitHub. Betas are refused: Flathub follows stable
// releases only.

import { createHash } from 'node:crypto'
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const [version, folder] = process.argv.slice(2)
if (!version || !folder) {
  console.error('usage: node tools/flatpak/update.mjs <version> <folder>')
  process.exit(1)
}
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`${version}: only stable versions go to Flathub`)
  process.exit(1)
}

const here = dirname(fileURLToPath(import.meta.url))
const id = 'com.synaelle.ficelle'

async function get(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(120_000) })
  if (!response.ok) {
    console.error(`${url}: ${response.status}`)
    process.exit(1)
  }
  return response
}

const release = await (
  await get(`https://api.github.com/repos/Synaelle/ficelle/releases/tags/v${version}`)
).json()
const date = release.published_at.slice(0, 10)
const appimage = await get(
  `https://github.com/Synaelle/ficelle/releases/download/v${version}/Ficelle_${version}_amd64.AppImage`,
)
const sha256 = createHash('sha256')
  .update(Buffer.from(await appimage.arrayBuffer()))
  .digest('hex')

const metainfoPath = join(here, `${id}.metainfo.xml`)
const metainfo = readFileSync(metainfoPath, 'utf8')
if (!metainfo.includes(`<release version="${version}"`)) {
  // newest first, as the list reads
  writeFileSync(
    metainfoPath,
    metainfo.replace(
      '<releases>\n',
      `<releases>\n    <release version="${version}" date="${date}"/>\n`,
    ),
  )
  console.log(`${version} added to ${id}.metainfo.xml, commit it`)
}

const manifest = readFileSync(join(here, `${id}.yml`), 'utf8')
  .replaceAll('@VERSION@', version)
  .replaceAll('@SHA256@', sha256)
mkdirSync(folder, { recursive: true })
writeFileSync(join(folder, `${id}.yml`), manifest)
copyFileSync(metainfoPath, join(folder, `${id}.metainfo.xml`))
copyFileSync(join(here, `${id}.desktop`), join(folder, `${id}.desktop`))
// only an x86_64 AppImage is released
writeFileSync(
  join(folder, 'flathub.json'),
  `${JSON.stringify({ 'only-arches': ['x86_64'] }, null, 2)}\n`,
)
console.log(`${id} ${version} written to ${folder}`)
