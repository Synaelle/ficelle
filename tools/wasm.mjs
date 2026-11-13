// SPDX-License-Identifier: AGPL-3.0-or-later
// Builds the Rust crates the pages carry as WebAssembly into
// apps/client/src/generated: the e2ee account client (crates/e2ee-web)
// always, and with --core the notes core of the web version
// (crates/core-web), which needs clang able to build for wasm32. Skips a
// crate when its output is newer than every source, unless the build has
// a release or preview identity. Fails fast, naming what is missing.

import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const BINDGEN = '0.2.129'

const crates = [
  {
    crate: 'app-e2ee-web',
    what: 'the e2ee client',
    out: 'e2ee',
    sources: ['crates/e2ee', 'crates/e2ee-web'],
  },
  ...(process.argv.includes('--core')
    ? [
        {
          crate: 'app-core-web',
          what: 'the notes core',
          out: 'core',
          sources: [
            'crates/core',
            'crates/core-web',
            'crates/e2ee',
            'crates/repo',
            'crates/protocol',
            'crates/brand',
            'Cargo.lock',
          ],
        },
      ]
    : []),
]

/** Newest modification time under `path`, in ms. @param {string} path @returns {number} */
function newest(path) {
  const info = statSync(path)
  if (!info.isDirectory()) return info.mtimeMs
  return Math.max(0, ...readdirSync(path).map((name) => newest(join(path, name))))
}

/** @param {string} command @param {string[]} args */
function run(command, args) {
  execFileSync(command, args, { cwd: root, stdio: 'inherit' })
}

// a release or a preview carries its own name and version (crates/brand):
// cargo decides then, it rebuilds only what that changes
const identity = process.env.APP_RELEASE === '1' || process.env.APP_PREVIEW === '1'

const stale = crates.filter(({ crate, out, sources }) => {
  if (identity) return true
  const marker = join(
    root,
    'apps/client/src/generated',
    out,
    `${crate.replaceAll('-', '_')}_bg.wasm`,
  )
  return (
    !existsSync(marker) ||
    statSync(marker).mtimeMs <= Math.max(...sources.map((s) => newest(join(root, s))))
  )
})
if (stale.length === 0) process.exit(0)

let version = ''
try {
  version = execFileSync('wasm-bindgen', ['--version'], { encoding: 'utf8' }).trim()
} catch {
  // reported below
}
if (!version.endsWith(BINDGEN)) {
  console.error(
    `wasm: needs wasm-bindgen ${BINDGEN} (found ${version || 'none'}).\n` +
      `  rustup target add wasm32-unknown-unknown\n` +
      `  cargo install wasm-bindgen-cli --version ${BINDGEN} --locked`,
  )
  process.exit(1)
}

for (const { crate, what, out } of stale) {
  console.log(`wasm: building ${what}`)
  run('cargo', ['build', '-q', '-p', crate, '--target', 'wasm32-unknown-unknown', '--release'])
  const dir = join(root, 'apps/client/src/generated', out)
  mkdirSync(dir, { recursive: true })
  const wasm = join(
    root,
    'target/wasm32-unknown-unknown/release',
    `${crate.replaceAll('-', '_')}.wasm`,
  )
  run('wasm-bindgen', ['--target', 'web', '--out-dir', dir, wasm])
  // written by the build, never committed
  writeFileSync(join(dir, '.gitignore'), '*\n')
}
console.log('wasm: done')
