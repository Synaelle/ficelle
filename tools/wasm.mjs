// SPDX-License-Identifier: AGPL-3.0-or-later
// Builds the e2ee account client (crates/e2ee-web) to WebAssembly for the
// web pages, into apps/client/src/generated/e2ee. Skips the work when the
// output is newer than every source. Fails fast, naming what is missing.

import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const out = join(root, 'apps/client/src/generated/e2ee')
const crate = 'app-e2ee-web'
const wasm = join(root, 'target/wasm32-unknown-unknown/release/app_e2ee_web.wasm')
const sources = [join(root, 'crates/e2ee'), join(root, 'crates/e2ee-web')]
const BINDGEN = '0.2.129'

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

const marker = join(out, 'app_e2ee_web_bg.wasm')
if (existsSync(marker) && statSync(marker).mtimeMs > Math.max(...sources.map(newest))) {
  process.exit(0)
}

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

console.log('wasm: building the e2ee client')
run('cargo', ['build', '-q', '-p', crate, '--target', 'wasm32-unknown-unknown', '--release'])
mkdirSync(out, { recursive: true })
run('wasm-bindgen', ['--target', 'web', '--out-dir', out, wasm])
// written by the build, never committed
writeFileSync(join(out, '.gitignore'), '*\n')
console.log('wasm: done')
