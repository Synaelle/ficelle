// SPDX-License-Identifier: AGPL-3.0-or-later
// Renders the beta builds' icons from packages/ui/brand/app-icon-beta.svg
// and app-icon-android-foreground-beta.svg, with the tauri cli's own
// renderer: the desktop set in src-tauri/icons-beta (tauri.beta.conf.json,
// picked by tools/build-release.mjs for a -beta.N version) and Android's
// launcher icons in gen/android/app/src/beta/res (picked by build.gradle.kts
// for a -beta.N versionName). Run again after editing either svg:
//
//   node tools/beta-icons.mjs

import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const brand = join(root, 'packages/ui/brand')
const tauri = join(root, 'apps/client/src-tauri')
const work = mkdtempSync(join(tmpdir(), 'beta-icons-'))

/**
 * Renders `svg` into `out`, the standard set or the given sizes.
 * @param {string} svg
 * @param {string} out
 * @param {number[]} [sizes]
 */
function render(svg, out, sizes) {
  const args = ['--filter', '@app/client', 'tauri', 'icon', join(brand, svg), '-o', out]
  if (sizes) args.push('-p', sizes.join(','))
  // pnpm is pnpm.cmd on Windows, which only a shell finds by that name
  const result = spawnSync('pnpm', args, { stdio: 'inherit', shell: process.platform === 'win32' })
  if (result.status !== 0) {
    console.error(`rendering ${svg} failed`)
    process.exit(1)
  }
}

/**
 * @param {string} from
 * @param {string} to
 */
const copy = (from, to) => {
  mkdirSync(dirname(to), { recursive: true })
  copyFileSync(from, to)
}

// the desktop set, the same files as icons-preview
const desk = join(work, 'desk')
render('app-icon-beta.svg', desk)
for (const file of [
  '32x32.png',
  '128x128.png',
  '128x128@2x.png',
  'icon.icns',
  'icon.ico',
  'icon.png',
])
  copy(join(desk, file), join(tauri, 'icons-beta', file))

// Android: the whole tile for older launchers, the foreground for adaptive
// icons, at each density
const densities = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 }
const launcher = join(work, 'launcher')
const foreground = join(work, 'foreground')
render(
  'app-icon-beta.svg',
  launcher,
  Object.values(densities).map((d) => 48 * d),
)
render(
  'app-icon-android-foreground-beta.svg',
  foreground,
  Object.values(densities).map((d) => 108 * d),
)
const res = join(tauri, 'gen/android/app/src/beta/res')
for (const [name, d] of Object.entries(densities)) {
  const tile = join(launcher, `${48 * d}x${48 * d}.png`)
  copy(tile, join(res, `mipmap-${name}`, 'ic_launcher.png'))
  copy(tile, join(res, `mipmap-${name}`, 'ic_launcher_round.png'))
  copy(
    join(foreground, `${108 * d}x${108 * d}.png`),
    join(res, `mipmap-${name}`, 'ic_launcher_foreground.png'),
  )
}

rmSync(work, { recursive: true, force: true })
console.log('beta icons written to icons-beta and src/beta/res')
