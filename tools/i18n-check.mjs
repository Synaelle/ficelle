// SPDX-License-Identifier: AGPL-3.0-or-later
// Lists translation problems. English (en.toml) is the reference.
// Fails on: keys missing from a language, placeholders that differ from
// English, and keys used in the code that English lacks.
// Warns on: keys no code uses, and keys a language has but English does not.
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
const isUsed = (/** @type {string} */ key) =>
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
for (const [code, strings] of Object.entries(locales)) {
  if (code === 'en') continue
  for (const [key, text] of Object.entries(english)) {
    if (!(key in strings)) errors.push(`${code}: missing ${key}`)
    else if (placeholders(text).join() !== placeholders(strings[key] ?? '').join())
      errors.push(`${code}: placeholders differ from English in ${key}`)
  }
  for (const key of Object.keys(strings)) {
    if (!(key in english)) warnings.push(`${code}: not in English, remove it: ${key}`)
  }
}

for (const w of warnings) console.warn(`warning  ${w}`)
for (const e of errors) console.error(`error    ${e}`)
const counts = Object.entries(locales)
  .map(([c, s]) => `${c} ${Object.keys(s).length}`)
  .join(', ')
console.log(`${counts} strings, ${errors.length} errors, ${warnings.length} warnings`)
process.exit(errors.length ? 1 : 0)
