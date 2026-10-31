// SPDX-License-Identifier: AGPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from 'vitest'
import { applyDyslexicFont, dyslexicCss, OPENDYSLEXIC } from './dyslexicFont'
import { dyslexicStatus } from './dyslexicStatus.svelte'
import { loadFontPack } from './fonts'

// nothing is downloaded in tests
vi.mock('./fonts', () => ({ loadFontPack: vi.fn(async () => true) }))

afterEach(() => {
  applyDyslexicFont(false)
  document.getElementById('theme-css')?.remove()
  localStorage.clear()
})

describe('the OpenDyslexic css', () => {
  const css = dyslexicCss('/m.woff2', 'abc123')

  it('loads the bundled mono file under a family only this page knows', () => {
    expect(css.match(/@font-face/g)).toHaveLength(1)
    expect(css).toContain("font-family: 'OpenDyslexic Mono abc123'; src: url('/m.woff2')")
  })

  it('forces the font as important inside its own layer, OpenDyslexic Mono for code', () => {
    expect(css).toContain('@layer a11y-abc123 {')
    expect(css).toMatch(/--font-ui: 'OpenDyslexic abc123'[^;]*!important/)
    expect(css).toMatch(/--font-editor: 'OpenDyslexic abc123'[^;]*!important/)
    expect(css).toMatch(/\* \{ font-family: 'OpenDyslexic abc123'[^;]*!important/)
    expect(css).toMatch(/\*::first-line \{ font-family: 'OpenDyslexic/)
    expect(css).toMatch(/--font-mono: 'OpenDyslexic Mono abc123'[^;]*!important/)
    expect(css).toMatch(/:where\(code, pre[^)]*\) \* \{ font-family: 'OpenDyslexic Mono abc123'/)
  })
})

describe('applyDyslexicFont', () => {
  it('puts its style before every other, a theme added later included', () => {
    applyDyslexicFont(true)
    const theme = document.createElement('style')
    theme.id = 'theme-css'
    theme.textContent = '@layer x { * { font-family: Comic !important } }'
    document.head.append(theme)
    const first = document.head.querySelector('style')
    expect(first?.id).toBe('dyslexic-font')
    expect(first?.textContent).toContain('@layer a11y-')
    expect(document.documentElement.hasAttribute('data-dyslexic-font')).toBe(true)
  })

  it('loads OpenDyslexic under the same random family the css names', async () => {
    applyDyslexicFont(true)
    const css = document.getElementById('dyslexic-font')?.textContent ?? ''
    const family = vi.mocked(loadFontPack).mock.lastCall?.[2]
    expect(vi.mocked(loadFontPack).mock.lastCall?.[0]).toBe(OPENDYSLEXIC)
    expect(family).toMatch(/^OpenDyslexic [0-9a-f]{16}$/)
    expect(css).toContain(`--font-ui: '${family}'`)
    await vi.waitFor(() => expect(dyslexicStatus.loading).toBe(false))
    expect(dyslexicStatus.failed).toBe(false)
  })

  it('says when the font could not be downloaded', async () => {
    vi.mocked(loadFontPack).mockRejectedValueOnce(new Error('offline'))
    applyDyslexicFont(true)
    await vi.waitFor(() => expect(dyslexicStatus.failed).toBe(true))
    applyDyslexicFont(false)
    expect(dyslexicStatus.failed).toBe(false)
  })
})
