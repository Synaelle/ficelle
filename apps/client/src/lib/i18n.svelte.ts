// SPDX-License-Identifier: AGPL-3.0-or-later
// Translations. Strings live in packages/locales/<code>.toml, English is the
// fallback. Placeholders are {name}, {appName} is always available. A key
// with .one/.other (and other CLDR plural forms) children is chosen by the
// {count} value with Intl.PluralRules.

import { brand } from '@app/brand'

// every packages/locales/<code>.toml: a language started on the translation
// website comes with no change here
const files = import.meta.glob<Record<string, string>>('../../../../packages/locales/*.toml', {
  eager: true,
  import: 'default',
})
const catalogs: Record<string, Record<string, string>> = Object.fromEntries(
  Object.entries(files).map(([path, strings]) => [
    path.slice(path.lastIndexOf('/') + 1, -'.toml'.length),
    strings,
  ]),
)
const english = catalogs.en ?? {}

// a text empty in English shows nothing, there is nothing to translate
const toTranslate = Object.keys(english).filter((key) => english[key] !== '')

/** Share of the English texts a language has, from 0 to 1. */
const translated = (code: string) =>
  toTranslate.filter((key) => key in (catalogs[code] ?? {})).length / toTranslate.length

// a page mostly in English with a few translated words reads worse than
// English alone, so a language is offered once half of it is done, with its
// share shown beside it
const READY = 0.5

/** The languages offered, each named in itself, with the share translated. */
export const LOCALES = Object.keys(catalogs)
  .filter((code) => code === 'en' || translated(code) >= READY)
  .map((code) => {
    const name = new Intl.DisplayNames([code], { type: 'language' }).of(code) ?? code
    return {
      code,
      name: name.charAt(0).toLocaleUpperCase(code) + name.slice(1),
      done: code === 'en' ? 1 : translated(code),
    }
  })
  .sort((a, b) => (a.code === 'en' ? -1 : b.code === 'en' ? 1 : a.name.localeCompare(b.name)))

/** Where anyone can help translate. */
export const LOCALES_URL = 'https://hosted.weblate.org/projects/ficelle/'

export type Locale = string

const offered = new Set(LOCALES.map((l) => l.code))

/** First offered language of the system, English otherwise. */
export function systemLocale(): Locale {
  for (const tag of navigator.languages ?? [navigator.language]) {
    const code = tag.slice(0, 2).toLowerCase()
    if (offered.has(code)) return code
  }
  return 'en'
}

const state = $state({ locale: 'en' as Locale })

/** Whether this build offers the language, a saved one may have been removed since. */
export const isLocale = (code: unknown): code is Locale =>
  typeof code === 'string' && offered.has(code)

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
    [state.locale, catalogs[state.locale] ?? english],
    ['en', english],
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

/** "92% translated" for a language not fully translated yet, else nothing. */
export function translatedLine(done: number): string | undefined {
  if (done >= 1) return undefined
  // rounded down, 99.6% is not finished
  const percent = new Intl.NumberFormat(state.locale, { style: 'percent' }).format(
    Math.floor(done * 100) / 100,
  )
  return t('settings.languageDone', { percent })
}
