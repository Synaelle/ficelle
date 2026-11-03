// SPDX-License-Identifier: AGPL-3.0-or-later
// Version, build date and channel shown to users. Same rules as
// crates/brand/build.rs, keep both in sync:
// APP_RELEASE=1 gives the workspace version, APP_PREVIEW=1 gives preview-{version}-{commit} (the release it is based on), CI gives dev-{commit},
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

/** Preview builds, for daily use before a release, share the real app identity and data. */
export function hasReleaseIdentity() {
  return isRelease() || process.env.APP_PREVIEW === '1'
}

export function releaseVersion() {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const match = cargo.match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)
  if (!match?.[1]) throw new Error('workspace version not found in Cargo.toml')
  return match[1]
}

export function appVersion() {
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
  if (!parts) throw new Error(`not a release version: ${version}`)
  const [major, minor, micro, beta] = parts
  if (minor > 999 || micro > 999 || (beta !== Infinity && (beta < 1 || beta > 98)))
    throw new Error(`no room in the version code for ${version}`)
  return (major * 1000 + minor) * 100000 + micro * 100 + (beta === Infinity ? 99 : beta)
}

/**
 * `major.minor.micro` and a beta's number, `Infinity` for a release, so they
 * order the way they come out. Null for anything else.
 * @param {string} version
 * @returns {[number, number, number, number] | null}
 */
function versionParts(version) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?$/.exec(version.trim())
  if (!m) return null
  return [Number(m[1]), Number(m[2]), Number(m[3]), m[4] === undefined ? Infinity : Number(m[4])]
}

/**
 * `a` comes out after `b`: numbers in order, a beta before its release.
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

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [command, a = '', b = ''] = process.argv.slice(2)
  // `node tools/version.mjs android-code` prints the release's version code
  if (command === 'android-code') console.log(androidVersionCode(releaseVersion()))
  // `node tools/version.mjs newer A B` exits 0 when A comes after B (or B is
  // not a version at all, like an empty one), else 1
  else if (command === 'newer') process.exit(isNewer(a, b) ? 0 : 1)
  else console.log(`${appVersion()} ${buildDate()}`)
}
