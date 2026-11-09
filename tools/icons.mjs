// SPDX-License-Identifier: AGPL-3.0-or-later
// Renders every app icon from packages/ui/brand with the tauri cli's own
// renderer: the release set in src-tauri/icons (betas use it too, they share
// the release's identity), the preview and dev sets beside it, and Android's
// launcher icons in gen/android/app/src/{main,preview,debug}/res. Run again
// after editing any of the svgs:
//
//   pnpm icons

import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const brand = join(root, 'packages/ui/brand')
const tauri = join(root, 'apps/client/src-tauri')
const work = mkdtempSync(join(tmpdir(), 'icons-'))

/**
 * Renders `svg` into `out`, the standard set or the given sizes.
 * @param {string} svg
 * @param {string} out
 * @param {number[]} [sizes]
 */
function render(svg, out, sizes) {
  console.log(`rendering ${svg}${sizes ? ` at ${sizes.join(', ')}` : ''}`)
  const args = ['--filter', '@app/client', 'tauri', 'icon', join(brand, svg), '-o', out]
  if (sizes) args.push('-p', sizes.join(','))
  // pnpm is pnpm.cmd on Windows, which only a shell finds by that name
  const result = spawnSync('pnpm', args, {
    stdio: ['ignore', 'ignore', 'inherit'],
    shell: process.platform === 'win32',
    timeout: 120_000,
  })
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

render('app-icon.svg', join(tauri, 'icons'))

// preview and dev: the files their tauri config names
for (const [svg, folder] of [
  ['app-icon-preview.svg', 'icons-preview'],
  ['app-icon-dev.svg', 'icons-dev'],
]) {
  const desk = join(work, folder)
  render(svg, desk)
  for (const file of [
    '32x32.png',
    '128x128.png',
    '128x128@2x.png',
    'icon.icns',
    'icon.ico',
    'icon.png',
  ])
    copy(join(desk, file), join(tauri, folder, file))
}

// Android: the knot alone for older launchers, the foreground of adaptive
// icons and, on the release, the monochrome layer of themed icons, which
// only uses the foreground's shape
const densities = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 }
for (const [set, launcherSvg, foregroundSvg] of [
  ['main', 'app-icon.svg', 'app-icon-android-foreground.svg'],
  ['preview', 'app-icon-preview.svg', 'app-icon-android-foreground-preview.svg'],
  ['debug', 'app-icon-dev.svg', 'app-icon-android-foreground-dev.svg'],
]) {
  const launcher = join(work, set, 'launcher')
  const foreground = join(work, set, 'foreground')
  render(
    launcherSvg,
    launcher,
    Object.values(densities).map((d) => 48 * d),
  )
  render(
    foregroundSvg,
    foreground,
    Object.values(densities).map((d) => 108 * d),
  )
  const res = join(tauri, 'gen/android/app/src', set, 'res')
  for (const [name, d] of Object.entries(densities)) {
    const mipmap = join(res, `mipmap-${name}`)
    const tile = join(launcher, `${48 * d}x${48 * d}.png`)
    const front = join(foreground, `${108 * d}x${108 * d}.png`)
    copy(tile, join(mipmap, 'ic_launcher.png'))
    copy(tile, join(mipmap, 'ic_launcher_round.png'))
    copy(front, join(mipmap, 'ic_launcher_foreground.png'))
    if (set === 'main') copy(front, join(mipmap, 'ic_launcher_monochrome.png'))
  }
}

// the tauri cli writes its own adaptive icon and a background picked from the
// svg into the Android project at each run: the tile colour and the
// monochrome layer go back
const mainRes = join(tauri, 'gen/android/app/src/main/res')
writeFileSync(
  join(mainRes, 'values/ic_launcher_background.xml'),
  `<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">#0F0F10</color>
</resources>
`,
)
writeFileSync(
  join(mainRes, 'mipmap-anydpi-v26/ic_launcher.xml'),
  `<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
  <background android:drawable="@color/ic_launcher_background"/>
  <monochrome android:drawable="@mipmap/ic_launcher_monochrome"/>
</adaptive-icon>
`,
)

rmSync(work, { recursive: true, force: true })
console.log('icons written to src-tauri/icons, icons-preview, icons-dev and the android res')
