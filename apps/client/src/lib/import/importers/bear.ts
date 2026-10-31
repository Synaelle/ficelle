// SPDX-License-Identifier: AGPL-3.0-or-later
// Bear: a backup (`.bear2bk`, a zip of `.textbundle` folders) or notes
// exported as text bundles or markdown. Each bundle has the text, an
// `info.json` with dates, pin, trash and archive, and its files in
// `assets/`. Tags are written in the text: `#tag`, `#a/b`, `#two words#`.

import { isMarkdown } from '../markdown'
import { baseName, dirName, readText } from '../source'
import type { Importer, ImportSource } from '../types'
import { hidden, inlineTags, readMarkdownNotes } from './markdown'

const BUNDLE_TEXT = /\.textbundle\/text\.(markdown|md|txt)$/i
const isBundleText = (path: string) => BUNDLE_TEXT.test(path)

interface BearInfo {
  created?: number
  updated?: number
  pinned?: boolean
  trashed?: boolean
  archived?: boolean
  id?: string
}

/** Seconds from 2001 (Apple's epoch), an ISO date, or nothing. */
function bearTime(value: unknown): number | undefined {
  if (typeof value === 'number') return Math.round((value + 978_307_200) * 1000)
  if (typeof value === 'string') {
    const time = Date.parse(value)
    return Number.isFinite(time) ? time : undefined
  }
  return undefined
}

const flag = (value: unknown) => value === true || value === 1 || value === '1'

async function readInfo(source: ImportSource, text: string): Promise<BearInfo> {
  const path = `${dirName(text)}/info.json`
  if (!source.entries.some((e) => e.path === path)) return {}
  try {
    const json = JSON.parse(await readText(source, path)) as Record<string, unknown>
    const bear = (json['net.shinyfrog.bear'] ?? {}) as Record<string, unknown>
    return {
      created: bearTime(bear.creationDate),
      updated: bearTime(bear.modificationDate),
      pinned: flag(bear.pinned),
      trashed: flag(bear.trashed),
      archived: flag(bear.archived),
      id: typeof bear.uniqueIdentifier === 'string' ? bear.uniqueIdentifier : undefined,
    }
  } catch {
    return {}
  }
}

/** `#two words#` tags, which plain inline tags do not see. */
export function bearTags(text: string): string[] {
  const words: string[] = []
  // taken out first, or `#two words#` would also give the tag `two`
  const rest = text.replace(
    /(^|\s)#([^\s#][^#\n]*?[^\s#])#(?=\s|$)/gm,
    (all, space: string, tag: string) => {
      if (!tag.includes(' ')) return all
      words.push(tag)
      return space
    },
  )
  return [...inlineTags(rest), ...words]
}

export const bearImporter: Importer = {
  id: 'bear',
  app: 'Bear',
  picks: ['file', 'folder'],
  extensions: ['bear2bk', 'bearbk', 'zip', 'textbundle'],
  detect: (source) => {
    if (/\.bear2?bk$/i.test(source.name)) return 0.95
    return source.entries.some((e) => isBundleText(e.path)) ? 0.9 : 0
  },
  read: async (source, report) => {
    const notes = source.entries
      .map((e) => e.path)
      .filter(
        (p) => (isBundleText(p) || (isMarkdown(p) && !p.includes('.textbundle/'))) && !hidden(p),
      )
    const infos = new Map<string, BearInfo>()
    let underlined = 0
    return readMarkdownNotes(source, report, {
      notes,
      // Bear has no folders
      folders: [],
      folderOf: () => null,
      titleOf: (path) =>
        baseName(isBundleText(path) ? dirName(path) : path).replace(
          /\.(textbundle|md|markdown)$/i,
          '',
        ),
      aliases: async (index) => {
        for (const path of notes) {
          if (isBundleText(path)) infos.set(path, await readInfo(source, path))
          const first = /^#\s+(.+)$/m.exec(
            (await readText(source, path).catch(() => '')).slice(0, 2000),
          )
          if (first?.[1]) index.alias(first[1].trim(), path)
        }
      },
      prepare: (text, path) => {
        const info = infos.get(path) ?? {}
        let body = text
          // layout hints Bear leaves after pictures
          .replace(/<!--\s*\{[^}]*\}\s*-->/g, '')
          // a single tilde is underline in Bear, there is none here
          .replace(/(^|[^~])~([^~\n]+)~(?!~)/g, (_all, before: string, inner: string) => {
            underlined++
            return `${before}${inner}`
          })
        if (underlined) {
          report.add({ kind: 'simplified', detail: 'underline' })
          underlined = 0
        }
        body = body.replace(/\r\n?/g, '\n')
        const tags = bearTags(body)
        if (info.archived) tags.push('archived')
        return {
          text: body,
          tags,
          created: info.created,
          updated: info.updated,
          pinned: info.pinned,
          trashed: info.trashed,
          key: info.id,
        }
      },
    })
  },
}
