// SPDX-License-Identifier: AGPL-3.0-or-later
// The notes core (crates/core-web) throws its errors as json in the message:
// read back here into the apps' { kind, message }.

export interface CoreError {
  kind: string
  message: string
}

/** The core's error as the apps send theirs. */
export function coreError(err: unknown): CoreError {
  const message = err instanceof Error ? err.message : String(err)
  try {
    const parsed: unknown = JSON.parse(message)
    if (parsed && typeof parsed === 'object' && 'kind' in parsed && 'message' in parsed)
      return parsed as CoreError
  } catch {
    // not one of the core's, a crash: said as it is
  }
  return { kind: 'unknown', message }
}
