// SPDX-License-Identifier: AGPL-3.0-or-later
// Fills the ficelle-bin PKGBUILD for a stable release and writes it with its
// .SRCINFO into a folder, ready to commit and push to the AUR repository
// (ssh://aur@aur.archlinux.org/ficelle-bin.git).
//
//   node tools/aur/update.mjs <version> <folder>
//
// The checksum comes from the release's own .deb, downloaded from GitHub.
// Betas are refused: the AUR package follows stable releases only.

import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const [version, folder] = process.argv.slice(2)
if (!version || !folder) {
  console.error('usage: node tools/aur/update.mjs <version> <folder>')
  process.exit(1)
}
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`${version}: only stable versions go to the AUR`)
  process.exit(1)
}

const here = dirname(fileURLToPath(import.meta.url))
const deb = `Ficelle_${version}_amd64.deb`
const url = `https://github.com/Synaelle/ficelle/releases/download/v${version}/${deb}`
const response = await fetch(url, { signal: AbortSignal.timeout(120_000) })
if (!response.ok) {
  console.error(`${url}: ${response.status}`)
  process.exit(1)
}
const sha256 = createHash('sha256')
  .update(Buffer.from(await response.arrayBuffer()))
  .digest('hex')

const pkgbuild = readFileSync(join(here, 'PKGBUILD'), 'utf8')
  .replaceAll('@VERSION@', version)
  .replaceAll('@SHA256@', sha256)
const field = (name) => {
  const match = pkgbuild.match(new RegExp(`^${name}=(.*)$`, 'm'))
  if (!match) throw new Error(`the PKGBUILD has no ${name}`)
  return match[1]
}
/** The values of a bash array or a single quoted value. */
const values = (raw) =>
  [...raw.matchAll(/'([^']*)'|"([^"]*)"/g)].map((m) =>
    (m[1] ?? m[2]).replaceAll('${pkgver}', version),
  )

const lines = [
  `pkgbase = ${field('pkgname')}`,
  `\tpkgdesc = ${values(field('pkgdesc'))[0]}`,
  `\tpkgver = ${version}`,
  `\tpkgrel = ${field('pkgrel')}`,
  `\turl = ${values(field('url'))[0]}`,
  ...values(field('arch')).map((v) => `\tarch = ${v}`),
  ...values(field('license')).map((v) => `\tlicense = ${v}`),
  ...values(field('depends')).map((v) => `\tdepends = ${v}`),
  ...values(field('provides')).map((v) => `\tprovides = ${v}`),
  ...values(field('conflicts')).map((v) => `\tconflicts = ${v}`),
  ...values(field('options')).map((v) => `\toptions = ${v}`),
  ...values(field('source')).map((v) => `\tsource = ${v}`),
  `\tsha256sums = ${sha256}`,
  '',
  `pkgname = ${field('pkgname')}`,
  '',
]

mkdirSync(folder, { recursive: true })
writeFileSync(join(folder, 'PKGBUILD'), pkgbuild)
writeFileSync(join(folder, '.SRCINFO'), lines.join('\n'))
console.log(`ficelle-bin ${version} written to ${folder}`)
