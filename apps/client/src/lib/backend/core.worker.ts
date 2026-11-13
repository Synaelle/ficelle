// SPDX-License-Identifier: AGPL-3.0-or-later
// The web version's notes core (crates/core-web), run here: the browser's
// private file storage it keeps the notes in is only reached from a worker,
// and long work (a password's key, an import) never freezes the page.
// Messages are { id, name, args, bytes?, binary? }, answers { id, value }
// or { id, error } with the apps' { kind, message } error.

import init, { call, callBytes, readBytes, start } from '@app/core-web'

export interface CoreRequest {
  id: number
  name: string
  /** The arguments as json: the page's own objects may not cross over. */
  args: string
  /** Sent with the command: a file to attach, a backup to restore. */
  bytes?: Uint8Array
  /** The answer is bytes: a file, a backup, an export. */
  binary?: boolean
}

export type CoreAnswer =
  { id: number; value: unknown } | { id: number; error: { kind: string; message: string } }

/** The core's error as the apps send theirs. */
function asError(err: unknown): { kind: string; message: string } {
  const message = err instanceof Error ? err.message : String(err)
  try {
    const parsed: unknown = JSON.parse(message)
    if (parsed && typeof parsed === 'object' && 'kind' in parsed && 'message' in parsed)
      return parsed as { kind: string; message: string }
  } catch {
    // not one of the core's, a crash: said as it is
  }
  return { kind: 'unknown', message }
}

const ready = init().then(() => start())

self.onmessage = async ({ data }: MessageEvent<CoreRequest>) => {
  const { id, name, args, bytes, binary } = data
  try {
    await ready
    if (binary) {
      const value = readBytes(name, args)
      self.postMessage({ id, value } satisfies CoreAnswer, { transfer: [value.buffer] })
      return
    }
    const text = bytes ? callBytes(name, args, bytes) : call(name, args)
    self.postMessage({ id, value: JSON.parse(text) as unknown } satisfies CoreAnswer)
  } catch (err) {
    self.postMessage({ id, error: asError(err) } satisfies CoreAnswer)
  }
}
