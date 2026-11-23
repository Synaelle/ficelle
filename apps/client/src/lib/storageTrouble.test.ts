// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { storageTrouble } from './storageTrouble'

/** A browser's storage: its folder given or refused, its room. */
function storage(folder: () => Promise<unknown>, usage = 0, quota = 1e9) {
  return {
    getDirectory: folder,
    estimate: async () => ({ usage, quota }),
  } as unknown as StorageManager
}
const reached = storage(async () => ({}))
const failed = { kind: 'storage', message: 'install failed' }

describe('storageTrouble', () => {
  it('tells a browser with no file storage', async () => {
    expect(await storageTrouble(failed, undefined)).toBe('missing')
    expect(await storageTrouble(failed, {} as StorageManager)).toBe('missing')
  })

  it('tells a private window from a site blocked', async () => {
    const insecure = storage(async () => {
      throw new DOMException('The operation is insecure.', 'SecurityError')
    })
    expect(await storageTrouble(failed, insecure)).toBe('private')
    const other = storage(async () => {
      throw new DOMException('denied', 'NotAllowedError')
    })
    expect(await storageTrouble(failed, other)).toBe('blocked')
  })

  it('tells a full storage', async () => {
    expect(await storageTrouble({ kind: 'file', message: 'QuotaExceededError' }, reached)).toBe(
      'full',
    )
    const nearly = storage(async () => ({}), 999_800, 1_000_000)
    expect(await storageTrouble(failed, nearly)).toBe('full')
  })

  it('tells the notes still open from before a reload', async () => {
    const held = {
      kind: 'storage',
      message:
        "acquire sync access handle: NoModificationAllowedError: Failed to execute 'createSyncAccessHandle' on 'FileSystemFileHandle': Access Handles cannot be created if there is another open Access Handle",
    }
    expect(await storageTrouble(held, reached)).toBe('busy')
  })

  it('blames the storage only for its own errors', async () => {
    expect(await storageTrouble(failed, reached)).toBe('blocked')
    expect(await storageTrouble({ kind: 'database', message: 'malformed' }, reached)).toBeNull()
  })
})
