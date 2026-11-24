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
})
