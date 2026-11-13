// SPDX-License-Identifier: AGPL-3.0-or-later

import { brand } from '@app/brand'
import { describe, expect, it } from 'vitest'
import fr from '../../../../packages/locales/fr.toml'
import { currentLocale, setLocale, t } from './i18n.svelte'

describe('t', () => {
  it('fills placeholders and the app name', async () => {
    await setLocale('en')
    expect(t('errorReport.lead')).toContain(brand.displayName)
    expect(t('tags.newInside', { name: 'Garden' })).toContain('Garden')
  })

  it('chooses plural forms with the rules of the language', async () => {
    await setLocale('en')
    expect(t('list.count', { count: 0 })).toBe('0 notes')
    expect(t('list.count', { count: 1 })).toBe('1 note')
    expect(t('list.count', { count: 2 })).toBe('2 notes')
    await setLocale('fr')
    expect(t('list.count', { count: 0 })).toBe('0 note')
    expect(t('list.count', { count: 1 })).toBe('1 note')
    expect(t('list.count', { count: 2 })).toBe('2 notes')
  })

  it('shows English, with English plurals, for a text not translated yet', async () => {
    const kept = { one: fr['list.count.one'] ?? '', other: fr['list.count.other'] ?? '' }
    delete fr['list.count.one']
    delete fr['list.count.other']
    try {
      await setLocale('fr')
      expect(t('list.count', { count: 0 })).toBe('0 notes')
      expect(t('list.count', { count: 1 })).toBe('1 note')
    } finally {
      fr['list.count.one'] = kept.one
      fr['list.count.other'] = kept.other
    }
  })

  it('shows the key when a text is missing everywhere', async () => {
    await setLocale('en')
    expect(t('nothing.here')).toBe('nothing.here')
  })

  it('falls back to English for a language this build does not have', async () => {
    await setLocale('de' as 'en')
    expect(currentLocale()).toBe('en')
    expect(t('errorReport.lead')).toContain(brand.displayName)
  })
})
