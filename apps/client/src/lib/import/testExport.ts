// SPDX-License-Identifier: AGPL-3.0-or-later
// Exports for the import tests, opened by the notes core as a picked file
// is: a tar of the files, so the pipeline writes through the same commands
// as with a real export.

import { keepPicked } from '../pick'
import { openSource } from './index'
import type { ImportSource } from './types'

const BLOCK = 512

/** `value` in octal, as wide as a tar header field leaves room for. */
function octal(value: number, width: number): string {
  return value.toString(8).padStart(width - 1, '0')
}

/** A ustar archive of `files` by path, each last changed at `modified`. */
function tar(
  files: Record<string, string | Uint8Array>,
  modified: number,
): Uint8Array<ArrayBuffer> {
  const encoder = new TextEncoder()
  const parts: Uint8Array[] = []
  for (const [path, content] of Object.entries(files)) {
    const data = typeof content === 'string' ? encoder.encode(content) : content
    const header = new Uint8Array(BLOCK)
    const put = (text: string, at: number) => header.set(encoder.encode(text), at)
    put(path, 0)
    put(octal(0o644, 8), 100)
    put(octal(0, 8), 108)
    put(octal(0, 8), 116)
    put(octal(data.length, 12), 124)
    put(octal(Math.floor(modified / 1000), 12), 136)
    // the checksum is counted with its own field as spaces
    put('        ', 148)
    put('0', 156)
    put('ustar\u000000', 257)
    put(
      `${octal(
        header.reduce((sum, b) => sum + b, 0),
        7,
      )}\u0000`,
      148,
    )
    parts.push(header, data, new Uint8Array((BLOCK - (data.length % BLOCK)) % BLOCK))
  }
  parts.push(new Uint8Array(2 * BLOCK))
  const out = new Uint8Array(parts.reduce((sum, p) => sum + p.length, 0))
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
}

/** An export of `files` named `name`, opened by the core. */
export function coreSource(
  name: string,
  files: Record<string, string | Uint8Array>,
  modified = Date.UTC(2025, 0, 1),
): Promise<ImportSource> {
  return openSource(keepPicked(new File([tar(files, modified)], `${name}.tar`)))
}
