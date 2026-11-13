// SPDX-License-Identifier: AGPL-3.0-or-later
// Lists translation problems. English (en.toml) is the reference.
// Fails on: placeholders that differ from English, and keys used in the code
// that English lacks.
// Warns on: keys a language has not translated yet (the app shows the English
// text meanwhile), keys no code uses, and keys a language has but English
// does not.
//
// Code usage is found from t('a.b') calls, and tr(lang, "a.b") in the server's
// rust. A template like t(`errors.${kind}`) marks every key under errors. as
// used.

import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { loadLocales, localesDir, placeholders } from './i18n.mjs'

const srcDir = join(localesDir, '..', '..', 'apps', 'client', 'src')

/** @param {string} dir @returns {string[]} */
function sourceFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) return sourceFiles(path)
    return /\.(svelte|ts)$/.test(name) && !name.endsWith('.test.ts') ? [path] : []
  })
}

const locales = loadLocales()
const english = locales.en
if (!english) throw new Error('en.toml is missing')

const used = new Set()
const prefixes = new Set()
for (const file of sourceFiles(srcDir)) {
  const code = readFileSync(file, 'utf8')
  for (const m of code.matchAll(/\bt\(\s*['"]([\w.-]+)['"]/g)) used.add(m[1])
  for (const m of code.matchAll(/\bt\(\s*`([\w.-]+)\$\{/g)) prefixes.add(m[1])
}
// the server's emails: tr(lang, "mail.reset.subject") in rust
const serverDir = join(localesDir, '..', '..', 'apps', 'server', 'src')
/** @param {string} dir @returns {string[]} */
function rustFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) return rustFiles(path)
    return name.endsWith('.rs') ? [path] : []
  })
}
for (const file of rustFiles(serverDir)) {
  const code = readFileSync(file, 'utf8')
  for (const m of code.matchAll(/\btr\(\s*\w+,\s*"([\w.-]+)"/g)) used.add(m[1])
}
// the web version's own wording of a text, see apps/client/src/lib/i18n.svelte.ts
/** @type {(key: string) => boolean} */
const isUsed = (key) =>
  (key.startsWith('inBrowser.') && isUsed(key.slice('inBrowser.'.length))) ||
  used.has(key) ||
  used.has(key.replace(/\.(zero|one|two|few|many|other)$/, '')) ||
  [...prefixes].some((p) => key.startsWith(p))

const errors = []
const warnings = []

for (const key of used) {
  const plural = Object.keys(english).some(
    (k) => k.startsWith(`${key}.`) && /\.(one|other)$/.test(k),
  )
  if (!(key in english) && !plural) errors.push(`en: used in code but missing: ${key}`)
}
for (const key of Object.keys(english)) {
  if (!isUsed(key)) warnings.push(`en: not used by any code: ${key}`)
}
const FORMS = ['zero', 'one', 'two', 'few', 'many', 'other']
/** `files.inNote` for `files.inNote.few` when English has that plural, else null. */
const pluralBase = (/** @type {string} */ key) => {
  const at = key.lastIndexOf('.')
  const base = key.slice(0, at)
  return FORMS.includes(key.slice(at + 1)) && `${base}.other` in english ? base : null
}
/** The plural forms a language needs beyond one and other. "many" only for
 * millions (French, Spanish) falls back to other well enough. */
const extraForms = (/** @type {string} */ code) => {
  const rules = new Intl.PluralRules(code)
  const small = new Set(Array.from({ length: 200 }, (_, n) => rules.select(n)))
  return rules
    .resolvedOptions()
    .pluralCategories.filter((form) => form !== 'one' && form !== 'other' && small.has(form))
}

for (const [code, strings] of Object.entries(locales)) {
  if (code === 'en') continue
  // a text empty in English shows nothing, there is nothing to translate
  const missing = Object.keys(english).filter((key) => english[key] !== '' && !(key in strings))
  // a language being started has most texts missing: one line, not hundreds
  if (missing.length > 50) warnings.push(`${code}: ${missing.length} texts not translated yet`)
  else for (const key of missing) warnings.push(`${code}: not translated yet: ${key}`)
  for (const [key, text] of Object.entries(english)) {
    if (key in strings && placeholders(text).join() !== placeholders(strings[key] ?? '').join())
      errors.push(`${code}: placeholders differ from English in ${key}`)
  }
  for (const key of Object.keys(strings)) {
    if (!(key in english) && !pluralBase(key))
      warnings.push(`${code}: not in English, remove it: ${key}`)
  }
  // Polish says 2 notatki but 5 notatek: its plurals need forms English has not
  for (const form of extraForms(code)) {
    for (const key of Object.keys(english)) {
      const base = key.endsWith('.other') && key.slice(0, -'.other'.length)
      if (base && `${base}.other` in strings && !(`${base}.${form}` in strings))
        warnings.push(`${code}: plural form missing: ${base}.${form}`)
    }
  }
}

// [list], then other sections, then [list.count] is valid TOML, but the
// translation website cannot read a section split like that
for (const file of readdirSync(localesDir).filter((f) => f.endsWith('.toml'))) {
  const seen = new Set()
  let current = ''
  for (const [, name] of readFileSync(join(localesDir, file), 'utf8').matchAll(/^\[([^\]]+)\]/gm)) {
    const top = name.split('.')[0]
    if (top !== current && seen.has(top))
      errors.push(`${file}: [${name}] is apart from the rest of [${top}]`)
    seen.add(top)
    current = top
  }
}

for (const w of warnings) console.warn(`warning  ${w}`)
for (const e of errors) console.error(`error    ${e}`)
const counts = Object.entries(locales)
  .map(([c, s]) => `${c} ${Object.keys(s).length}`)
  .join(', ')
console.log(`${counts} strings, ${errors.length} errors, ${warnings.length} warnings`)
process.exit(errors.length ? 1 : 0)
