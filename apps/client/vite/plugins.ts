// SPDX-License-Identifier: AGPL-3.0-or-later
// Small vite plugins for things that should not ship a runtime parser.

import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import type { Plugin } from 'vite'
import { parseLocale } from '../../../tools/i18n.mjs'
import { LUCIDE, PHOSPHOR } from '../src/lib/icons/sets.ts'

/** `import strings from 'x.toml'` gives the flat "section.key" strings. */
export function tomlLocales(): Plugin {
  return {
    name: 'toml-locales',
    transform(source, id) {
      if (!id.endsWith('.toml')) return null
      return { code: `export default ${JSON.stringify(parseLocale(source))}`, map: null }
    },
  }
}

const VIRTUAL_ICONS = 'virtual:icons'

/** Keeps only the paths of an svg file, sizing is done by the Icon component. */
function svgInner(file: string): string {
  const text = readFileSync(file, 'utf8').replace(/<!--[\s\S]*?-->/g, '')
  const match = /<svg[^>]*>([\s\S]*)<\/svg>/.exec(text)
  if (!match?.[1]) throw new Error(`not an svg: ${file}`)
  return match[1].replace(/\s+/g, ' ').trim()
}

/**
 * `virtual:icons` exports the inner markup of the icons listed in
 * src/lib/icons/sets.ts, and nothing else of the icon packages.
 */
export function iconSets(): Plugin {
  const require = createRequire(import.meta.url)
  const resolved = '\0' + VIRTUAL_ICONS
  return {
    name: 'icon-sets',
    resolveId: (id) => (id === VIRTUAL_ICONS ? resolved : null),
    load(id) {
      if (id !== resolved) return null
      const read = (map: Record<string, string>, path: (name: string) => string) =>
        Object.fromEntries(
          Object.entries(map).map(([role, name]) => [role, svgInner(require.resolve(path(name)))]),
        )
      const lucide = read(LUCIDE, (n) => `lucide-static/icons/${n}.svg`)
      const phosphor = read(PHOSPHOR, (n) => `@phosphor-icons/core/assets/regular/${n}.svg`)
      return `export const lucide = ${JSON.stringify(lucide)}\nexport const phosphor = ${JSON.stringify(phosphor)}\n`
    },
  }
}
