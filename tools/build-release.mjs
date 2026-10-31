// SPDX-License-Identifier: AGPL-3.0-or-later
// Builds the desktop app with the release version and identity. A plain
// tauri build gives the dev identity, see crates/brand/build.rs.
// --preview keeps the release identity but labels it preview-{commit}, for
// using the app day to day while it is still in development.
// --portable builds the portable Windows exe (src-tauri/src/portable.rs),
// numbered like the release, copied to target/release/bundle/portable/.
// Build it after the installer: both use target/release/ficelle.exe. With
// --preview too it is a preview's portable exe, Ficelle_preview_x64_portable.exe.
// --unsigned builds the release without the updater's signed copy, for a
// test build without the key (the Linux workflow's manual runs).
// Anything after -- goes to tauri build, like --bundles appimage.

import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const preview = process.argv.includes('--preview')
const portable = process.argv.includes('--portable')
const unsigned = process.argv.includes('--unsigned')
const dash = process.argv.indexOf('--')
const passed = dash > 0 ? process.argv.slice(dash + 1) : []
// a release installer comes with the signed copy the app updater downloads,
// tauri reads the key from here (the key itself or its file)
if (!preview && !portable && !unsigned && !process.env.TAURI_SIGNING_PRIVATE_KEY) {
  console.error('TAURI_SIGNING_PRIVATE_KEY is not set: the key that signs app updates')
  process.exit(1)
}
if (portable && process.platform !== 'win32') {
  console.error('the portable exe is built on Windows')
  process.exit(1)
}
const channel = {
  ...(preview ? { APP_PREVIEW: '1' } : { APP_RELEASE: '1' }),
  ...(portable ? { APP_PORTABLE: '1' } : {}),
}
// a preview is run straight from the exe, an installer would only slow it
// down; the portable exe is the exe itself
const bundle = preview || portable ? ['--no-bundle'] : []
const overlays = ['src-tauri/tauri.release.conf.json']
if (portable) overlays.push('src-tauri/tauri.portable.conf.json')
// a preview's yellow knot, so it is never taken for the release
if (preview) overlays.push('src-tauri/tauri.preview.conf.json')
if (unsigned) {
  // a file, not the JSON inline: the Windows shell drops its quotes
  const file = join(root, 'target', 'unsigned.conf.json')
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, JSON.stringify({ bundle: { createUpdaterArtifacts: false } }))
  overlays.push(file)
}

const result = spawnSync(
  'pnpm',
  [
    '--filter',
    '@app/client',
    'tauri',
    'build',
    ...overlays.flatMap((file) => ['--config', file]),
    ...bundle,
    ...passed,
  ],
  {
    stdio: 'inherit',
    shell: process.platform === 'win32',
    env: { ...process.env, ...channel },
  },
)
if (result.status !== 0) process.exit(result.status ?? 1)

if (portable) {
  const brand = JSON.parse(readFileSync(join(root, 'packages/brand/brand.json'), 'utf8'))
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const version = cargo.match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)?.[1]
  const folder = join(root, 'target/release/bundle/portable')
  mkdirSync(folder, { recursive: true })
  const file = join(
    folder,
    `${brand.displayName}_${preview ? 'preview' : version}_x64_portable.exe`,
  )
  copyFileSync(join(root, 'target/release', `${brand.slug}.exe`), file)
  console.log(`portable exe: ${file}`)
}
