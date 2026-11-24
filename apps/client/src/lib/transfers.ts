// SPDX-License-Identifier: AGPL-3.0-or-later
// Files on their way to or from the server, as the screens say them: the
// Sync screen's "Sending 2 files, 120 of 450 MB", and a file's own
// progress beside it.

import { bytesOf } from './format'
import { t } from './i18n.svelte'
import type { Moving } from './sync.svelte'

/** The file with this hash as it moves, null while it does not. */
export function movingOf(moving: Moving[], sha256: string): Moving | null {
  return moving.find((f) => f.sha256 === sha256) ?? null
}

/** A line for the files sent, then one for the files received, each only
 * while some move. */
export function movingLines(moving: Moving[]): string[] {
  const lines: string[] = []
  for (const sending of [true, false]) {
    const files = moving.filter((f) => f.sending === sending)
    if (files.length === 0) continue
    const done = files.reduce((sum, f) => sum + f.done, 0)
    const total = files.reduce((sum, f) => sum + f.total, 0)
    lines.push(
      t(sending ? 'sync.sending' : 'sync.receiving', {
        count: files.length,
        progress: bytesOf(done, total),
      }),
    )
  }
  return lines
}
