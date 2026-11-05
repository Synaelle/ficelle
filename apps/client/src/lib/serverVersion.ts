// SPDX-License-Identifier: AGPL-3.0-or-later
// How this app's version and sync protocol compare with a server's. Only the
// first two numbers of a version count: the last moves on its own for fixes
// and server-only releases, a beta counts as the version it leads to. A
// different version is advice, only a protocol the two do not share stops sync.

/** The first two numbers of a version, null when it reads as something else. */
function series(version: string): [number, number] | null {
  const found = /^v?(\d+)\.(\d+)(?:\D|$)/.exec(version.trim())
  return found ? [Number(found[1]), Number(found[2])] : null
}

/** Which of the two is behind, null when they match or one cannot be read. */
export function versionGap(app: string, server: string): 'serverOlder' | 'serverNewer' | null {
  const a = series(app)
  const s = series(server)
  if (!a || !s) return null
  const difference = s[0] - a[0] || s[1] - a[1]
  if (difference === 0) return null
  return difference < 0 ? 'serverOlder' : 'serverNewer'
}

/** Which side needs an update for the two to talk, null when they can. */
export function protocolGap(
  app: number,
  min: number | undefined,
  max: number | undefined,
): 'updateApp' | 'updateServer' | null {
  if (min !== undefined && app < min) return 'updateApp'
  if (max !== undefined && app > max) return 'updateServer'
  return null
}

/** A server's protocol versions, `1` or `1–3`, null when it did not say. */
export function protocolRange(min: number | undefined, max: number | undefined): string | null {
  if (min === undefined || max === undefined) return null
  return min === max ? String(min) : `${min}–${max}`
}
