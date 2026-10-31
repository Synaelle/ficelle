// SPDX-License-Identifier: AGPL-3.0-or-later

import { brand } from '@app/brand'
import { describe, expect, it } from 'vitest'
import { currentLocale, setLocale, t } from './i18n.svelte'

describe('t', () => {
  it('fills placeholders and the app name', () => {
    setLocale('en')
    expect(t('setup.title')).toBe(`Welcome to ${brand.displayName}`)
    expect(t('folders.confirmTrash', { name: 'Garden' })).toContain('Garden')
  })

  it('chooses plural forms with the rules of the language', () => {
    setLocale('en')
    expect(t('list.count', { count: 0 })).toBe('0 notes')
    expect(t('list.count', { count: 1 })).toBe('1 note')
    expect(t('list.count', { count: 2 })).toBe('2 notes')
  })

  it('shows the key when a text is missing everywhere', () => {
    setLocale('en')
    expect(t('nothing.here')).toBe('nothing.here')
  })

  it('falls back to English for a language this build does not have', () => {
    setLocale('fr' as 'en')
    expect(currentLocale()).toBe('en')
    expect(t('setup.title')).toBe(`Welcome to ${brand.displayName}`)
  })
})
