// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { bytes } from './format'
import { setLocale } from './i18n.svelte'

describe('bytes', () => {
  it('says a small file in bytes, not as 0 kB', async () => {
    await setLocale('en')
    expect(bytes(33)).toBe('33 bytes')
    expect(bytes(1)).toBe('1 byte')
  })

  it('says larger files in kB and up', async () => {
    await setLocale('en')
    expect(bytes(1536)).toBe('1.5 kB')
    expect(bytes(30 * 1024 * 1024)).toBe('30 MB')
  })
})
