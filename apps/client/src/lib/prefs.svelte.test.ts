// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { prefs } from './prefs.svelte'

/** Stores `saved` as another window of the app would, which loads it here. */
function storedElsewhere(saved: Record<string, unknown>) {
  localStorage.setItem('prefs', JSON.stringify(saved))
  window.dispatchEvent(new StorageEvent('storage', { key: 'prefs' }))
}

describe('prefs', () => {
  it('keeps a language the app has', () => {
    storedElsewhere({ language: 'fr' })
    expect(prefs.language).toBe('fr')
  })

  it('follows the system again for a language the app no longer has', () => {
    for (const gone of ['de', 'es', 'it', 'nl', 'pt']) {
      storedElsewhere({ language: gone })
      expect(prefs.language).toBeNull()
    }
  })

  it('uses the default font again for one the app no longer offers', () => {
    storedElsewhere({ font: 'roboto', codeFont: 'firaMono' })
    expect(prefs.font).toBe('default')
    expect(prefs.codeFont).toBe('default')
    storedElsewhere({ font: 'ubuntu', codeFont: 'jetbrainsMono' })
    expect(prefs.font).toBe('ubuntu')
    expect(prefs.codeFont).toBe('jetbrainsMono')
  })

  it('keeps Ubuntu picked while fonts were tried on nightlies, not the others', () => {
    storedElsewhere({ trialFont: 'ubuntu', trialMono: 'robotoMono' })
    expect(prefs.font).toBe('ubuntu')
    expect(prefs.codeFont).toBe('default')
    storedElsewhere({ trialFont: 'lato' })
    expect(prefs.font).toBe('default')
  })
})
