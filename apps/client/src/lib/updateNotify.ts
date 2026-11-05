// SPDX-License-Identifier: AGPL-3.0-or-later
// Whether a found update is worth a system notification. Off by default, once
// per version, not while the app is in front (its card says it already), and
// never for a version the person chose to skip. A critical fix keeps its prompt
// in the app and is told here too: the window may be out of sight.

const NOTIFIED_KEY = 'updateNotified'

export function worthNotifying(
  update: { version: string; critical?: boolean },
  state: { on: boolean; inFront: boolean; notified: string | null; skipped: string | null },
): boolean {
  if (!state.on || state.inFront || update.version === state.notified) return false
  return !!update.critical || update.version !== state.skipped
}

export function lastNotified(): string | null {
  try {
    return localStorage.getItem(NOTIFIED_KEY)
  } catch {
    return null
  }
}

export function rememberNotified(version: string) {
  try {
    localStorage.setItem(NOTIFIED_KEY, version)
  } catch {
    // told again on the next start
  }
}
