// SPDX-License-Identifier: AGPL-3.0-or-later
// Version, build date and channel shown to users. Same rules as
// crates/brand/build.rs, keep both in sync:
// APP_RELEASE=1 gives the workspace version, or with APP_NIGHTLY the nightly's name (nightly-2026-11-16) and APP_BUILD its build number,
// APP_PREVIEW=1 gives preview-{version}-{commit} (the release it is based on), CI gives dev-{commit},
// anything else gives dev-{commit} with -dirty when the tree has changes.
// APP_COMMIT and APP_COMMIT_DATE stand in for git where the sources come
// without their history (the Docker image).

import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

// fixed so versions never change width as the repo grows
const HASH_LEN = 9

/** @param {string[]} args */
function git(args) {
  try {
    return execFileSync('git', args, { cwd: root, stdio: ['ignore', 'pipe', 'ignore'] })
      .toString()
      .trim()
  } catch {
    return ''
  }
}

export function isRelease() {
  return process.env.APP_RELEASE === '1'
}

/** Release and preview builds: release settings (updates). A preview keeps the
 * dev tools and short passwords, it is for testing, and has a name, id and
 * data of its own, see packages/brand. */
export function hasReleaseIdentity() {
  return isRelease() || process.env.APP_PREVIEW === '1'
}

export function releaseVersion() {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const match = cargo.match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)
  if (!match?.[1]) throw new Error('workspace version not found in Cargo.toml')
  return match[1]
}

/** A nightly's name, `nightly-2026-11-16`, `.2` and on for a fix the same day. */
const NIGHTLY = /^nightly-\d{4}-\d{2}-\d{2}(\.\d+)?$/

export function appVersion() {
  const nightly = process.env.APP_NIGHTLY?.trim()
  if (isRelease() && nightly) {
    if (!NIGHTLY.test(nightly)) throw new Error(`not a nightly name: ${nightly}`)
    return nightly
  }
  if (isRelease()) return releaseVersion()
  const commit =
    process.env.APP_COMMIT?.trim().slice(0, HASH_LEN) ||
    git(['rev-parse', `--short=${HASH_LEN}`, 'HEAD']) ||
    'unknown'
  if (process.env.APP_PREVIEW === '1') return `preview-${releaseVersion()}-${commit}`
  if (process.env.CI) return `dev-${commit}`
  const dirty = git(['status', '--porcelain']) !== ''
  return `dev-${commit}${dirty ? '-dirty' : ''}`
}

/** Date of the commit being built, so rebuilding the same commit gives the same output. */
export function buildDate() {
  return process.env.APP_COMMIT_DATE?.trim() || git(['show', '-s', '--format=%cs', 'HEAD'])
}

/**
 * Android's version code: it only updates to a higher one, and a beta comes
 * before its release. `(major * 1000 + minor) * 100000 + micro * 100`, plus
 * the beta's number, or 99 for the release: 0.4.0-beta.1 is 400001, 0.4.0
 * is 400099, both above the old scheme's 0.3.3 (3003).
 */
/** @param {string} version */
export function androidVersionCode(version) {
  const parts = versionParts(version)
  if (!parts) throw new Error(`not an app version: ${version}`)
  const [major, minor, micro, , beta] = parts
  if (minor > 999 || micro > 999 || (beta !== Infinity && (beta < 1 || beta > 98)))
    throw new Error(`no room in the version code for ${version}`)
  return (major * 1000 + minor) * 100000 + micro * 100 + (beta === Infinity ? 99 : beta)
}

/** Before a release come its betas. */
const STAGE = { beta: 1, release: 2 }

/**
 * `major.minor.micro`, the stage and its number (`Infinity` for a release),
 * so they order the way they come out. Null for anything else.
 * @param {string} version
 * @returns {[number, number, number, number, number] | null}
 */
function versionParts(version) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-(beta)\.(\d+))?$/.exec(version.trim())
  if (!m) return null
  const stage = m[4] === 'beta' ? STAGE.beta : STAGE.release
  const n = m[5] === undefined ? Infinity : Number(m[5])
  return [Number(m[1]), Number(m[2]), Number(m[3]), stage, n]
}

/**
 * `a` comes out after `b`: numbers in order, betas before their release.
 * @param {string} a
 * @param {string} b
 */
export function isNewer(a, b) {
  const [x, y] = [versionParts(a), versionParts(b)]
  if (!x) return false
  if (!y) return true
  const i = x.findIndex((n, k) => n !== y[k])
  return i !== -1 && (x[i] ?? 0) > (y[i] ?? 0)
}

/**
 * The build number of the `n`th nightly after `release`: its version code
 * plus `n`. Nightlies take the numbers betas had, between a release and the
 * next (0.6.0 is 600099, 0.6.1 is 600199), so the first nightly after a
 * release numbers above every nightly before it. 99 at most, then a release
 * is due.
 * @param {string} release
 * @param {number} n
 */
export function nightlyBuild(release, n) {
  if (!Number.isInteger(n) || n < 1 || n > 99)
    throw new Error(`nightly ${n} after ${release}: only 99 fit before the next release, make one`)
  const parts = versionParts(release)
  if (!parts || parts[3] !== STAGE.release) throw new Error(`not a release: ${release}`)
  return androidVersionCode(release) + n
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [command, a = '', b = ''] = process.argv.slice(2)
  // `node tools/version.mjs android-code` prints the build's version code: a
  // nightly's APP_BUILD, else the release's
  if (command === 'android-code')
    console.log(
      process.env.APP_NIGHTLY
        ? Number(process.env.APP_BUILD)
        : androidVersionCode(releaseVersion()),
    )
  // `node tools/version.mjs nightly-build N` prints the Nth nightly's number
  // after the workspace's release
  else if (command === 'nightly-build') console.log(nightlyBuild(releaseVersion(), Number(a)))
  // `node tools/version.mjs newer A B` exits 0 when A comes after B (or B is
  // not a version at all, like an empty one), else 1
  else if (command === 'newer') process.exit(isNewer(a, b) ? 0 : 1)
  else console.log(`${appVersion()} ${buildDate()}`)
}
