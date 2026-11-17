// SPDX-License-Identifier: AGPL-3.0-or-later
// Translates the texts a language file does not have yet with DeepL, and
// writes the file again in the order of en.toml. Texts already in the file
// stay as they are, so corrections made by hand are never replaced.
//
//   DEEPL_AUTH_KEY=... node tools/deepl.mjs de es
//   node tools/deepl.mjs --dry de      counts the characters, sends nothing
//
// Words in {braces} are sent as tags DeepL leaves alone, and the result is
// checked to hold the same ones as English.

import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { localesDir, parseLocale, placeholders } from './i18n.mjs'

// DeepL's codes where they differ from the file name
const TARGETS = { pt: 'PT-BR', en: 'EN-US', zh: 'ZH-HANS' }
const BATCH = 50

const args = process.argv.slice(2)
const dry = args.includes('--dry')
const codes = args.filter((a) => a !== '--dry')
if (codes.length === 0) throw new Error('usage: node tools/deepl.mjs [--dry] <code>...')
const key = process.env.DEEPL_AUTH_KEY
if (!dry && !key) throw new Error('DEEPL_AUTH_KEY is not set')
// free keys end in :fx and have their own address
const api = key?.endsWith(':fx') ? 'https://api-free.deepl.com' : 'https://api.deepl.com'

const englishSource = readFileSync(join(localesDir, 'en.toml'), 'utf8')
const english = parseLocale(englishSource)

const escapeXml = (/** @type {string} */ s) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
const unescapeXml = (/** @type {string} */ s) =>
  s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')
const toTags = (/** @type {string} */ s) => escapeXml(s).replace(/\{(\w+)\}/g, '<x>{$1}</x>')
const fromTags = (/** @type {string} */ s) => unescapeXml(s.replace(/<x>(\{\w+\})<\/x>/g, '$1'))

/**
 * @param {string[]} texts
 * @param {string} target
 * @param {string} section
 * @returns {Promise<string[]>}
 */
async function translate(texts, target, section) {
  const response = await fetch(`${api}/v2/translate`, {
    method: 'POST',
    headers: { Authorization: `DeepL-Auth-Key ${key}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      text: texts.map(toTags),
      source_lang: 'EN',
      target_lang: target,
      tag_handling: 'xml',
      ignore_tags: ['x'],
      preserve_formatting: true,
      // one form of address throughout, as the French texts use vous
      formality: 'prefer_more',
      context: `Interface texts of Ficelle, a notes app, from its "${section}" part. Short labels are buttons, menu items or headings.`,
    }),
    signal: AbortSignal.timeout(60_000),
  })
  if (!response.ok) throw new Error(`DeepL answered ${response.status}: ${await response.text()}`)
  const { translations } = /** @type {{ translations: { text: string }[] }} */ (
    await response.json()
  )
  return translations.map((t) => fromTags(t.text))
}

for (const code of codes) {
  const path = join(localesDir, `${code}.toml`)
  const current = parseLocale(readFileSync(path, 'utf8'))
  const todo = Object.keys(english).filter((k) => english[k] !== '' && !(k in current))
  const chars = todo.reduce((sum, k) => sum + (english[k]?.length ?? 0), 0)
  console.log(`${code}: ${todo.length} texts, ${chars} characters to translate`)
  if (dry || todo.length === 0) continue

  /** @type {Record<string, string>} */
  const done = { ...current }
  const target = TARGETS[/** @type {keyof typeof TARGETS} */ (code)] ?? code.toUpperCase()
  // one section per request at most, so each gets its own context
  /** @type {Record<string, string[]>} */
  const groups = {}
  for (const k of todo) (groups[k.split('.')[0] ?? ''] ??= []).push(k)
  let sent = 0
  for (const [section, keys] of Object.entries(groups)) {
    for (let i = 0; i < keys.length; i += BATCH) {
      const batch = keys.slice(i, i + BATCH)
      const result = await translate(
        batch.map((k) => english[k] ?? ''),
        target,
        section,
      )
      batch.forEach((k, j) => {
        const text = result[j] ?? ''
        if (placeholders(text).join() !== placeholders(english[k] ?? '').join())
          console.warn(`${code}: placeholders changed in ${k}, left in English`)
        else done[k] = text
      })
      sent += batch.length
      console.log(`${code}: ${sent}/${todo.length}`)
    }
  }

  // the same sections, keys and order as en.toml, without its comments
  let section = ''
  let header = ''
  const lines = [`# ${new Intl.DisplayNames(['en'], { type: 'language' }).of(code)}, see en.toml`]
  /** @param {string} raw @param {string} text */
  const write = (raw, text) => {
    if (header) lines.push('', header)
    header = ''
    lines.push(`${raw} = ${JSON.stringify(text)}`)
  }
  for (const line of englishSource.split('\n')) {
    const table = line.match(/^\[([^\]]+)\]$/)
    if (table) {
      section = table[1] ?? ''
      header = line
      continue
    }
    const entry = line.match(/^("[^"]+"|[\w-]+) = /)
    if (!entry) continue
    const raw = entry[1] ?? ''
    const text = done[`${section}.${raw.replace(/^"|"$/g, '')}`]
    if (text !== undefined) write(raw, text)
    // forms English does not have, like Polish few and many, follow other
    if (raw === 'other')
      for (const form of ['zero', 'two', 'few', 'many']) {
        const extra = done[`${section}.${form}`]
        if (extra !== undefined && !(`${section}.${form}` in english)) write(form, extra)
      }
  }
  writeFileSync(path, `${lines.join('\n')}\n`)
  console.log(`${code}: written`)
}
