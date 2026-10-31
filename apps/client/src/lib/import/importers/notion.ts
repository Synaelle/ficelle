// SPDX-License-Identifier: AGPL-3.0-or-later
// Notion's "Markdown & CSV" export: a zip (zips inside it for large
// workspaces) of pages named `Title <32 hex id>.md`, each page's subpages
// in a folder of the same name, databases as CSV beside a folder of their
// rows. The id is the page's key, the title its first heading (file names
// are cut short), the properties under it give dates and tags.

import { isMarkdown, takeHeading } from '../markdown'
import { baseName, dirName, extension, withoutExtension } from '../source'
import type { Importer, ModelFolder } from '../types'
import { readMarkdownNotes } from './markdown'

const ID = /\s([0-9a-f]{32})$/i
export const notionId = (path: string) =>
  ID.exec(withoutExtension(baseName(path)))?.[1]?.toLowerCase()
/** A name without its id: `Plans 1a2b...` is `Plans`. */
export const withoutId = (name: string) => name.replace(ID, '').trim() || name

const isNotionNote = (path: string) =>
  (isMarkdown(path) || (extension(path) === 'csv' && !/_all\.csv$/i.test(path))) && !!notionId(path)

/** A CSV file as rows of cells, quotes and line breaks in cells handled. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let quoted = false
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i]
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"'
        i += 1
      } else if (c === '"') quoted = false
      else cell += c
    } else if (c === '"') quoted = true
    else if (c === ',') {
      row.push(cell)
      cell = ''
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i += 1
      row.push(cell)
      rows.push(row)
      row = []
      cell = ''
    } else cell += c
  }
  if (cell || row.length) rows.push([...row, cell])
  return rows.filter((r) => r.some((c) => c.trim()))
}

/** Rows as a markdown table. */
export function csvTable(rows: string[][]): string {
  const [head, ...body] = rows
  if (!head) return ''
  const cell = (text = '') => text.replace(/\|/g, '\\|').replace(/\n/g, ' ').trim() || ' '
  const line = (cells: string[]) => `| ${head.map((_, i) => cell(cells[i])).join(' | ')} |`
  return [line(head), `| ${head.map(() => '---').join(' | ')} |`, ...body.map(line)].join('\n')
}

const DATE_KEYS = /^(created|created time|date created|created at)$/i
const EDITED_KEYS = /^(last edited time|last edited|edited|updated|last updated)$/i
const TAG_KEYS = /^(tags?|labels?|keywords)$/i

/** The `Key: Value` lines under a page's title, Notion's properties. */
export function notionProperties(body: string): {
  body: string
  created?: number
  updated?: number
  tags: string[]
} {
  const lines = body.replace(/^\n+/, '').split('\n')
  const props: [string, string][] = []
  for (const line of lines) {
    const prop = /^([^:*#>\-[`|]{1,40}): (.+)$/.exec(line)
    if (!prop) break
    props.push([(prop[1] ?? '').trim(), (prop[2] ?? '').trim()])
  }
  if (!props.length) return { body, tags: [] }
  const time = (value?: string) => {
    const parsed = value ? Date.parse(value.replace(/\s*\([^)]*\)$/, '')) : NaN
    return Number.isFinite(parsed) ? parsed : undefined
  }
  const list = props.map(([key, value]) => `- **${key}:** ${value}`).join('\n')
  return {
    body: `${list}\n\n${lines.slice(props.length).join('\n').replace(/^\n+/, '')}`,
    created: time(props.find(([k]) => DATE_KEYS.test(k))?.[1]),
    updated: time(props.find(([k]) => EDITED_KEYS.test(k))?.[1]),
    tags: props
      .filter(([k]) => TAG_KEYS.test(k))
      .flatMap(([, v]) => v.split(','))
      .map((t) => t.trim())
      .filter(Boolean),
  }
}

/** Folders as Notion makes them: a page's subpages beside it. */
export function notionFolders(notes: string[]): {
  folders: ModelFolder[]
  folderOf: (path: string) => string | null
} {
  // zips inside the export are how Notion splits it, not folders
  const packaging = (dir: string) => /\.zip$/i.test(baseName(dir))
  const dirs = new Set<string>()
  for (const path of notes) {
    for (let dir = dirName(path); dir && !dirs.has(dir); dir = dirName(dir)) {
      if (!packaging(dir)) dirs.add(dir)
    }
  }
  /** The nearest folder kept above a path. */
  const above = (path: string): string | null => {
    for (let dir = dirName(path); dir; dir = dirName(dir)) if (dirs.has(dir)) return dir
    return null
  }
  const folders = [...dirs]
    .sort((a, b) => a.split('/').length - b.split('/').length || a.localeCompare(b))
    .map((dir) => ({ key: dir, parent: above(dir), name: withoutId(baseName(dir)) }))
  // a page goes in the folder of its subpages, so the two are one
  const folderOf = (path: string) => {
    const own = withoutExtension(path)
    return dirs.has(own) ? own : above(path)
  }
  return { folders, folderOf }
}

export const notionImporter: Importer = {
  id: 'notion',
  app: 'Notion',
  picks: ['file', 'folder'],
  extensions: ['zip'],
  detect: (source) => {
    const md = source.entries.filter((e) => isMarkdown(e.path) && notionId(e.path)).length
    const html = source.entries.filter(
      (e) => extension(e.path) === 'html' && notionId(e.path),
    ).length
    return md > 0 && md >= html ? 0.9 : 0
  },
  read: (source, report) => {
    const notes = source.entries.map((e) => e.path).filter(isNotionNote)
    const { folders, folderOf } = notionFolders(notes)
    return readMarkdownNotes(source, report, {
      notes,
      folders,
      folderOf,
      titleOf: (path) => withoutId(withoutExtension(baseName(path))),
      prepare: (text, path) => {
        const key = notionId(path)
        if (extension(path) === 'csv') {
          const title = withoutId(withoutExtension(baseName(path)))
          report.add({ kind: 'simplified', note: title, detail: 'database' })
          return { text: csvTable(parseCsv(text)), title, key: key && `csv:${key}` }
        }
        const heading = takeHeading(text)
        const props = notionProperties(heading.title ? heading.body : text)
        return {
          text: props.body,
          title: heading.title ?? undefined,
          tags: props.tags,
          created: props.created,
          updated: props.updated,
          key,
        }
      },
    })
  },
}
