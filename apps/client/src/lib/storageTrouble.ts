// SPDX-License-Identifier: AGPL-3.0-or-later
// Why the web version could not open its notes, as far as the page can tell:
// the browser's private file storage (OPFS), which the notes live in, is
// missing, refused in a private window, full, or blocked for this site. A
// browser asking for permission to store keeps it blocked until it is given.
// Or busy: the page before a reload still has the notes open for a moment.
// Refused is told as the page starts (webStorage.ts), never here.

export type StorageTrouble = 'busy' | 'missing' | 'private' | 'full' | 'blocked' | 'refused'

/** Less room than this left for the site counts as full. */
const ROOM = 1024 * 1024

/** What stopped `err`, the core's {kind, message}, or null when it is not
 * the storage. */
export async function storageTrouble(
  err: unknown,
  storage: StorageManager | undefined = navigator.storage,
): Promise<StorageTrouble | null> {
  const { kind = '', message = '' } = (err ?? {}) as { kind?: string; message?: string }
  if (/quota/i.test(message)) return 'full'
  // the browser lets one page at a time open a file of the notes
  if (/NoModificationAllowedError/.test(message)) return 'busy'
  if (typeof storage?.getDirectory !== 'function') return 'missing'
  try {
    await storage.getDirectory()
  } catch (refused) {
    // Firefox refuses it in a private window, with this name
    return refused instanceof DOMException && refused.name === 'SecurityError'
      ? 'private'
      : 'blocked'
  }
  if (await storageFull(0, storage)) return 'full'
  // the page reaches the storage, the worker could not open or write it
  return kind === 'storage' || kind === 'file' ? 'blocked' : null
}

/** Whether the site's storage has no room left for a write of about `size`
 * bytes, which SQLite writes more than once: its journal, the search index
 * and the note's versions. */
export async function storageFull(
  size: number,
  storage: StorageManager | undefined = navigator.storage,
): Promise<boolean> {
  try {
    const { usage = 0, quota } = (await storage?.estimate()) ?? {}
    return quota !== undefined && quota - usage < ROOM + 4 * size
  } catch {
    // no estimate: the other signs decide
    return false
  }
}
