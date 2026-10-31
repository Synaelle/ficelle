// SPDX-License-Identifier: AGPL-3.0-or-later
// A Logseq graph folder: pages and journals written as outlines, `key::
// value` properties, pages linked by name (namespaces `a/b` stored as
// `a___b.md`), `#[[tags]]`, tasks (`TODO`, `DONE`), and references to
// blocks by id, replaced by the block's text.

import { baseName, extension, readText, withoutExtension } from '../source'
import type { Importer, ImportSource, Report } from '../types'
import { readMarkdownNotes } from './markdown'

const isPage = (path: string) =>
  /^(?:.*\/)?(pages|journals)\/[^/]+\.md$/i.test(path) && !isOwn(path)
/** The app's own files: settings, backups, older versions. */
const isOwn = (path: string) => /(^|\/)logseq\//.test(path) || /(^|\/)\.recycle\//.test(path)
const isJournal = (path: string) => /(^|\/)journals\/[^/]+$/i.test(path)

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const ordinal = (day: number) =>
  `${day}${day % 10 === 1 && day !== 11 ? 'st' : day % 10 === 2 && day !== 12 ? 'nd' : day % 10 === 3 && day !== 13 ? 'rd' : 'th'}`

/** A journal's date from its file name, `2025_10_27.md`. */
function journalDate(path: string): { y: number; m: number; d: number } | null {
  const parts = /(\d{4})[_-](\d{2})[_-](\d{2})\.md$/.exec(baseName(path))
  if (!parts) return null
  return { y: Number(parts[1]), m: Number(parts[2]), d: Number(parts[3]) }
}

/** A page's name from its file name, as Logseq writes it. */
export function pageName(path: string): string {
  const date = isJournal(path) ? journalDate(path) : null
  if (date) return `${date.y}-${String(date.m).padStart(2, '0')}-${String(date.d).padStart(2, '0')}`
  let name = withoutExtension(baseName(path)).replace(/___/g, '/')
  try {
    name = decodeURIComponent(name)
  } catch {
    // kept as written
  }
  return name
}

/** Page properties: `key:: value` lines at the top of the page. */
function pageProperties(text: string): { props: Record<string, string>; rest: string } {
  const lines = text.split('\n')
  const props: Record<string, string> = {}
  let i = 0
  for (; i < lines.length; i += 1) {
    const prop = /^\s*(?:- )?([\w-]+):: ?(.*)$/.exec(lines[i] ?? '')
    if (!prop) break
    props[(prop[1] ?? '').toLowerCase()] = (prop[2] ?? '').trim()
  }
  return { props, rest: lines.slice(i).join('\n') }
}

/** `a, [[b c]], #d` to names. */
const listOf = (value = '') =>
  value
    .split(',')
    .map((v) =>
      v
        .trim()
        .replace(/^#/, '')
        .replace(/^\[\[(.*)\]\]$/, '$1')
        .trim(),
    )
    .filter(Boolean)

/** Block ids (`id:: uuid` under a block) to the block's first line. */
async function blockTexts(source: ImportSource, pages: string[]): Promise<Map<string, string>> {
  const blocks = new Map<string, string>()
  for (const path of pages) {
    const lines = (await readText(source, path).catch(() => '')).split(/\r?\n/)
    let last = ''
    for (const line of lines) {
      const id = /^\s*id:: ([0-9a-f-]{36})\s*$/i.exec(line)
      if (id) blocks.set((id[1] ?? '').toLowerCase(), last)
      else if (/^\s*- /.test(line))
        last = line.replace(/^\s*- (TODO|DONE|LATER|NOW|DOING) /, '').replace(/^\s*- /, '')
    }
  }
  return blocks
}

/** A page's text as plain markdown, references to blocks replaced. */
export function cleanPage(
  text: string,
  title: string,
  blocks: Map<string, string>,
  report: Report,
): { text: string; tags: string[] } {
  const tags: string[] = []
  const out = text
    // block properties are the app's own (ids, collapsed state)
    .replace(/^[ \t]*[\w-]+:: .*(?:\n|$)/gm, (line) => {
      const tag = /^\s*tags:: (.*)$/.exec(line.trimEnd())
      if (tag) tags.push(...listOf(tag[1]))
      return ''
    })
    .replace(/^(\s*- )(TODO|LATER|NOW|DOING|WAITING) /gm, '$1[ ] ')
    .replace(/^(\s*- )(DONE) /gm, '$1[x] ')
    .replace(/^(\s*- )CANCEL(?:L)?ED (.*)$/gm, '$1~~$2~~')
    .replace(
      /\{\{embed \(\(([0-9a-f-]{36})\)\)\}\}|\(\(([0-9a-f-]{36})\)\)/gi,
      (all, a?: string, b?: string) => {
        const found = blocks.get((a ?? b ?? '').toLowerCase())
        if (found === undefined) {
          report.add({ kind: 'linkMissing', note: title, detail: all })
          return ''
        }
        return found
      },
    )
    .replace(/\{\{embed \[\[([^\]]+)\]\]\}\}/g, '[[$1]]')
    .replace(/\{\{[^}\n]*\}\}/g, (all) => {
      report.add({ kind: 'dropped', note: title, detail: all })
      return ''
    })
    .replace(/(^|\s)#\[\[([^\]\n]+)\]\]/g, (_all, space: string, name: string) => {
      tags.push(name)
      return `${space}[[${name}]]`
    })
  return { text: out, tags }
}

export const logseqImporter: Importer = {
  id: 'logseq',
  app: 'Logseq',
  picks: ['folder', 'file'],
  extensions: ['zip'],
  detect: (source) => {
    const paths = source.entries.map((e) => e.path)
    if (paths.some((p) => /(^|\/)logseq\/config\.edn$/.test(p))) return 0.95
    return paths.some((p) => /(^|\/)journals\/\d{4}_\d{2}_\d{2}\.md$/.test(p)) ? 0.6 : 0
  },
  read: async (source, report) => {
    const paths = source.entries.map((e) => e.path)
    const pages = paths.filter(isPage)
    for (const path of paths.filter((p) => extension(p) === 'org' && !isOwn(p))) {
      report.add({ kind: 'unreadable', detail: path })
    }
    const blocks = await blockTexts(source, pages)
    const hasJournals = pages.some(isJournal)
    return readMarkdownNotes(source, report, {
      notes: pages,
      inlineTags: true,
      folders: hasJournals ? [{ key: 'journals', parent: null, name: 'Journals' }] : [],
      folderOf: (path) => (isJournal(path) ? 'journals' : null),
      titleOf: pageName,
      aliases: (index) => {
        for (const path of pages) {
          index.alias(pageName(path), path)
          const date = isJournal(path) ? journalDate(path) : null
          // the date formats people pick most for their journal titles
          if (date) {
            const month = MONTHS[date.m - 1] ?? ''
            index.alias(`${month} ${ordinal(date.d)}, ${date.y}`, path)
            index.alias(
              `${date.y}/${String(date.m).padStart(2, '0')}/${String(date.d).padStart(2, '0')}`,
              path,
            )
            index.alias(
              `${date.y}_${String(date.m).padStart(2, '0')}_${String(date.d).padStart(2, '0')}`,
              path,
            )
          }
        }
      },
      prepare: (text, path) => {
        const { props, rest } = pageProperties(text)
        const title = props.title || pageName(path)
        const page = cleanPage(rest, title, blocks, report)
        const date = isJournal(path) ? journalDate(path) : null
        return {
          text: page.text.replace(/^\s*-\s*$/gm, '').replace(/\n{3,}/g, '\n\n'),
          title,
          tags: [...listOf(props.tags), ...page.tags],
          created: date ? new Date(date.y, date.m - 1, date.d).getTime() : undefined,
        }
      },
    })
  },
}
