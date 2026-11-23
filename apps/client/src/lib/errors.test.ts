// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from 'vitest'
import { errorCode, errorLine, namedError, shownError } from './errors'
import { setLocale } from './i18n.svelte'

describe('shownError', () => {
  it('makes a code of where and what, in capitals', () => {
    expect(errorCode('SAVE', 'database')).toBe('SAVE-DATABASE')
    expect(errorCode('RESTORE', 'notBackup')).toBe('RESTORE-NOT-BACKUP')
    expect(errorCode('START', 'storage_busy')).toBe('START-STORAGE-BUSY')
  })

  it('says a known kind in words', async () => {
    await setLocale('en')
    expect(shownError({ kind: 'notBackup', message: 'not a backup' }, 'RESTORE')).toEqual({
      kind: 'notBackup',
      reason: 'This file is not a backup.',
      code: 'RESTORE-NOT-BACKUP',
    })
    expect(namedError('notMarkdown', 'IMPORT').code).toBe('IMPORT-NOT-MARKDOWN')
  })

  it('gives an unexpected error its message, on one line', async () => {
    await setLocale('en')
    const crash = shownError(new TypeError('x is\n  undefined'), 'SAVE')
    expect(crash.code).toBe('SAVE-UNKNOWN')
    expect(crash.reason).toContain('x is undefined')
    expect(crash.reason).not.toBe('An unexpected error happened.')
    // a kind with no text of its own is unexpected too, its message said
    const odd = shownError({ kind: 'internal', message: 'task panicked' }, 'SYNC')
    expect(odd.code).toBe('SYNC-INTERNAL')
    expect(odd.reason).toContain('task panicked')
    // nothing to say: the plain lead, never a blank
    expect(shownError(null, 'APP').reason).toBe('An unexpected error happened.')
  })

  it('keeps the code out of the reason', async () => {
    await setLocale('en')
    const full = shownError({ kind: 'storageFull', message: 'quota' }, 'SAVE')
    expect(full.reason).not.toContain('SAVE-STORAGE-FULL')
    expect(errorLine(full)).toBe(`${full.reason} Error code SAVE-STORAGE-FULL`)
  })
})
