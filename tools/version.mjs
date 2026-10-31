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

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  console.log(`${appVersion()} ${buildDate()}`)
}
