// SPDX-License-Identifier: AGPL-3.0-or-later
// Shared helpers for the translation files in packages/locales. Each file is
// TOML so translators can read comments, it is flattened to "section.key"
// strings at build time, the app never parses TOML itself.

import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parse } from 'smol-toml'

export const localesDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'packages', 'locales')

/**
 * Nested tables to flat dotted keys. Only strings are allowed as values.
 * @param {Record<string, unknown>} table
 * @param {string} [prefix]
 * @returns {Record<string, string>}
 */
export function flatten(table, prefix = '') {
  /** @type {Record<string, string>} */
  const out = {}
  for (const [key, value] of Object.entries(table)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (typeof value === 'string') out[path] = value
    else if (value && typeof value === 'object' && !Array.isArray(value))
      Object.assign(out, flatten(/** @type {Record<string, unknown>} */ (value), path))
    else throw new Error(`${path}: translations must be text`)
  }
  return out
}

/** @param {string} source */
export function parseLocale(source) {
  return flatten(parse(source))
}

/** Every locale file as { code: flat strings }. */
export function loadLocales() {
  /** @type {Record<string, Record<string, string>>} */
  const all = {}
  for (const file of readdirSync(localesDir).filter((f) => f.endsWith('.toml'))) {
    all[file.replace(/\.toml$/, '')] = parseLocale(readFileSync(join(localesDir, file), 'utf8'))
  }
  return all
}

/** Placeholders like {count} used in a string, sorted. */
export function placeholders(/** @type {string} */ text) {
  return [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()
}
