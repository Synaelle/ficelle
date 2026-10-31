// SPDX-License-Identifier: AGPL-3.0-or-later
// The instance's rules as the apps apply them.

import { describe, expect, it } from 'vitest'
import type { Extensions } from './backend/types'
import { themeAllowed } from './instance.svelte'
import { prefs } from './prefs.svelte'

const ext = (rules: Partial<Extensions['rules']>, themes: string[] = []): Extensions => ({
  rules: { themes: 'any', ...rules },
  themes: themes.map((id) => ({
    id,
    name: id,
    size: 1,
    sha256: `t-${id}`,
    preinstall: false,
    uploadedAt: 0,
  })),
})

describe('themes under the instance rules', () => {
  it('any imported theme without a server or under "any"', () => {
    expect(themeAllowed('mine', null)).toBe(true)
    expect(themeAllowed('mine', ext({}))).toBe(true)
  })

  it('under "only the instance\'s", only the ones added from it', () => {
    prefs.instanceThemes = { ocean: 't-ocean' }
    const rules = ext({ themes: 'instance' }, ['ocean'])
    expect(themeAllowed('ocean', rules)).toBe(true)
    expect(themeAllowed('mine', rules)).toBe(false)
    expect(themeAllowed('ocean', ext({ themes: 'instance' }, []))).toBe(false)
    prefs.instanceThemes = {}
  })
})

describe('the official catalog served by the instance', () => {
  it('counts its themes as offered, once added from it', () => {
    const rules = ext({ themes: 'instance' })
    prefs.instanceThemes = {}
    expect(themeAllowed('dusk', rules, ['dusk'])).toBe(false)
    prefs.instanceThemes = { dusk: 'x' }
    expect(themeAllowed('dusk', rules, ['dusk'])).toBe(true)
    expect(themeAllowed('dusk', rules, [])).toBe(false)
    prefs.instanceThemes = {}
  })
})
