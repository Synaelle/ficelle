// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it, vi } from 'vitest'
import { applyFont, FONT_PACKS, fontStatus } from './chosenFonts.svelte'
import { loadFontPack } from './fonts'

// nothing is downloaded in tests
vi.mock('./fonts', () => ({ loadFontPack: vi.fn(async () => true) }))

describe('applyFont', () => {
  it('uses the default at once, with nothing to download', () => {
    applyFont('text', 'default')
    expect(fontStatus.text).toEqual({ family: null, loading: false, error: null })
  })

  it('keeps the default while a font downloads, then uses it', async () => {
    let arrive: (ok: boolean) => void = () => {}
    vi.mocked(loadFontPack).mockReturnValueOnce(new Promise((r) => (arrive = r)))
    applyFont('code', 'jetbrainsMono')
    expect(fontStatus.code).toEqual({ family: null, loading: true, error: null })
    expect(vi.mocked(loadFontPack).mock.lastCall).toEqual([FONT_PACKS.jetbrainsMono, true])
    arrive(true)
    await vi.waitFor(() => expect(fontStatus.code.family).toBe('JetBrains Mono'))
    expect(fontStatus.code.loading).toBe(false)
  })

  it('says why a font did not come, keeping the default, and tries again', async () => {
    vi.mocked(loadFontPack).mockRejectedValueOnce({ kind: 'offline', message: 'no network' })
    applyFont('text', 'ubuntu')
    await vi.waitFor(() => expect(fontStatus.text.error?.code).toBe('FONT-OFFLINE'))
    expect(fontStatus.text.family).toBeNull()
    applyFont('text', 'ubuntu')
    expect(fontStatus.text.error).toBeNull()
    await vi.waitFor(() => expect(fontStatus.text.family).toBe('Ubuntu'))
  })

  it('ignores a download that ends after the pick changed', async () => {
    let arrive: (ok: boolean) => void = () => {}
    vi.mocked(loadFontPack).mockReturnValueOnce(new Promise((r) => (arrive = r)))
    applyFont('text', 'ubuntu')
    applyFont('text', 'default')
    arrive(true)
    await Promise.resolve()
    expect(fontStatus.text.family).toBeNull()
  })
})
