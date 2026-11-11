// SPDX-License-Identifier: AGPL-3.0-or-later
// Translations. Strings live in packages/locales/<code>.toml, English is the
// fallback. Placeholders are {name}, {appName} is always available. A key
// with .one/.other (and other CLDR plural forms) children is chosen by the
// {count} value with Intl.PluralRules.

import { brand } from '@app/brand'
import en from '../../../../packages/locales/en.toml'
import fr from '../../../../packages/locales/fr.toml'

export const LOCALES = [
  { code: 'en', name: 'English' },
  { code: 'fr', name: 'Français' },
] as const

/** Where anyone can correct a translation. */
export const LOCALES_URL = 'https://github.com/Synaelle/ficelle/tree/main/packages/locales'

export type Locale = (typeof LOCALES)[number]['code']

const catalogs: Record<Locale, Record<string, string>> = { en, fr }

/** First supported language of the system, English otherwise. */
export function systemLocale(): Locale {
  for (const tag of navigator.languages ?? [navigator.language]) {
    const code = tag.slice(0, 2).toLowerCase()
    if (code in catalogs) return code as Locale
  }
  return 'en'
}

const state = $state({ locale: 'en' as Locale })

/** Whether this build has the language, a saved one may have been removed since. */
export const isLocale = (code: unknown): code is Locale =>
  typeof code === 'string' && Object.hasOwn(catalogs, code)

/** Falls back to English for a language this build no longer has. */
export function setLocale(locale: Locale) {
  const known = isLocale(locale) ? locale : 'en'
  state.locale = known
  document.documentElement.lang = known
}

export function currentLocale(): Locale {
  return state.locale
}

type Vars = Record<string, string | number>

function lookup(key: string, vars: Vars): string | undefined {
  // a language saved by an older version may be gone; a text not translated
  // yet comes in English, its plural chosen by English rules
  const tables: [Locale, Record<string, string>][] = [
    catalogs[state.locale] ? [state.locale, catalogs[state.locale]] : ['en', catalogs.en],
    ['en', catalogs.en],
  ]
  if (typeof vars.count === 'number') {
    for (const [locale, table] of tables) {
      const form = new Intl.PluralRules(locale).select(vars.count)
      const text = table[`${key}.${form}`] ?? table[`${key}.other`]
      if (text !== undefined) return text
    }
  }
  for (const [, table] of tables) {
    if (table[key] !== undefined) return table[key]
  }
  return undefined
}

/** Translated text for `key`, or the key itself so a gap is visible, never blank. */
export function t(key: string, vars: Vars = {}): string {
  const text = lookup(key, vars) ?? key
  const all: Vars = { appName: brand.displayName, ...vars }
  return text.replace(/\{(\w+)\}/g, (match, name: string) =>
    all[name] === undefined ? match : String(all[name]),
  )
}
