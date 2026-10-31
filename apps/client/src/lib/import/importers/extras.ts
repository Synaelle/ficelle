// SPDX-License-Identifier: AGPL-3.0-or-later
// Smaller formats: outlines in OPML (Workflowy, Dynalist), Day One's json,
// Roam Research's json, and Tomboy and Gnote's `.note` files.

import { parseHtml } from '../html'
import { NoteFiles, takeHeading } from '../markdown'
import { baseName, dirName, extension, readText, withoutExtension } from '../source'
import {
  fileRef,
  type Importer,
  type ImportSource,
  type ModelFolder,
  type ModelNote,
  type Report,
} from '../types'
import { inlineTags } from './markdown'

const plain = (html: string) => parseHtml(html).body.textContent?.trim() ?? ''
const time = (value: unknown) => {
  if (typeof value === 'number') return value
  const parsed = typeof value === 'string' ? Date.parse(value.replace(/(\.\d{3})\d+/, '$1')) : NaN
  return Number.isFinite(parsed) ? parsed : undefined
}

async function readXml(
  source: ImportSource,
  path: string,
  report: Report,
): Promise<Document | null> {
  try {
    const xml = new DOMParser().parseFromString(await readText(source, path), 'application/xml')
    if (!xml.querySelector('parsererror')) return xml
  } catch {
    // reported below
  }
  report.add({ kind: 'unreadable', detail: path })
  return null
}

/** An outline as nested list items, done items as checked tasks. */
function outlineMarkdown(outlines: Element[], depth = 0): string {
  return outlines
    .map((o) => {
      const text = plain(o.getAttribute('text') ?? '')
      const done = o.getAttribute('_complete') === 'true'
      const indent = '  '.repeat(depth)
      const note = o.getAttribute('_note')
      const lines = [`${indent}- ${done ? '[x] ' : ''}${text}`]
      if (note) lines.push(...note.split('\n').map((l) => `${indent}  ${l}`))
      const children = [...o.children].filter((c) => c.localName === 'outline')
      if (children.length) lines.push(outlineMarkdown(children, depth + 1))
      return lines.join('\n')
    })
    .join('\n')
}

export const opmlImporter: Importer = {
  id: 'opml',
  app: 'Workflowy, Dynalist (OPML)',
  picks: ['file', 'folder'],
  extensions: ['opml', 'xml', 'zip'],
  detect: (source) => (source.entries.some((e) => extension(e.path) === 'opml') ? 0.9 : 0),
  read: async (source, report) => {
    const notes: ModelNote[] = []
    const modified = new Map(source.entries.map((e) => [e.path, e.modified ?? undefined]))
    for (const path of source.entries.map((e) => e.path).filter((p) => extension(p) === 'opml')) {
      const xml = await readXml(source, path, report)
      if (!xml) continue
      const body = xml.querySelector('body')
      const tops = body ? [...body.children].filter((c) => c.localName === 'outline') : []
      const title =
        xml.querySelector('head > title')?.textContent?.trim() || withoutExtension(baseName(path))
      notes.push({
        key: path,
        folder: null,
        title,
        body: outlineMarkdown(tops),
        tags: [],
        created: modified.get(path),
        updated: modified.get(path),
        files: [],
      })
    }
    return { folders: [], notes }
  },
}

interface DayOneEntry {
  uuid?: string
  text?: string
  creationDate?: string
  modifiedDate?: string
  tags?: string[]
  starred?: boolean
  photos?: { identifier?: string; md5?: string; type?: string }[]
  videos?: { identifier?: string; md5?: string; type?: string }[]
  audios?: { identifier?: string; md5?: string; type?: string }[]
  pdfAttachments?: { identifier?: string; md5?: string; type?: string }[]
}

const dayOneJournals = (source: ImportSource) =>
  source.entries
    .map((e) => e.path)
    .filter((p) => extension(p) === 'json' && !p.includes('/photos/'))

export const dayOneImporter: Importer = {
  id: 'day-one',
  app: 'Day One',
  picks: ['file', 'folder'],
  extensions: ['zip', 'json'],
  detect: (source) => {
    const paths = source.entries.map((e) => e.path)
    if (!paths.some((p) => extension(p) === 'json')) return 0
    return paths.some(
      (p) => /(^|\/)(photos|videos|audios|pdfs)\//.test(p) || /(^|\/)Journal\.json$/.test(p),
    )
      ? 0.8
      : 0.1
  },
  read: async (source, report) => {
    const paths = source.entries.map((e) => e.path)
    const journals = dayOneJournals(source)
    const folders: ModelFolder[] =
      journals.length > 1
        ? journals.map((p) => ({ key: p, parent: null, name: withoutExtension(baseName(p)) }))
        : []
    const notes: ModelNote[] = []
    for (const journal of journals) {
      let entries: DayOneEntry[]
      try {
        entries =
          (JSON.parse(await readText(source, journal)) as { entries?: DayOneEntry[] }).entries ?? []
      } catch {
        report.add({ kind: 'unreadable', detail: journal })
        continue
      }
      const dir = dirName(journal)
      for (const entry of entries) {
        const files = new NoteFiles()
        const media = new Map<string, string>()
        const kinds = [
          ['photos', entry.photos],
          ['videos', entry.videos],
          ['audios', entry.audios],
          ['pdfs', entry.pdfAttachments],
        ] as const
        for (const [folder, items] of kinds) {
          for (const item of items ?? []) {
            const prefix = `${dir ? `${dir}/` : ''}${folder}/${item.md5 ?? ''}`
            const found = paths.find((p) => p.startsWith(prefix))
            if (found && item.identifier) media.set(item.identifier, files.add(found))
          }
        }
        const text = (entry.text ?? '').replace(/\r\n?/g, '\n')
        const heading = takeHeading(text)
        const firstLine = text.split('\n')[0] ?? ''
        const title = heading.title ?? firstLine.slice(0, 120)
        let body = heading.title ? heading.body : text.slice(firstLine.length).replace(/^\n+/, '')
        body = body.replace(
          /!\[([^\]]*)\]\(dayone-moment:\/*(?:\w+\/)?([\w-]+)\)/g,
          (all, alt: string, id: string) => {
            const name = media.get(id)
            if (!name) {
              report.add({ kind: 'fileMissing', note: title, detail: id })
              return ''
            }
            return `![${alt}](${fileRef(name)})`
          },
        )
        notes.push({
          key: entry.uuid ?? `${journal}:${notes.length}`,
          folder: folders.length ? journal : null,
          title,
          body,
          tags: entry.tags ?? [],
          created: time(entry.creationDate),
          updated: time(entry.modifiedDate),
          pinned: entry.starred === true,
          files: files.files,
        })
      }
    }
    return { folders, notes }
  },
}

interface RoamBlock {
  string?: string
  uid?: string
  children?: RoamBlock[]
}
interface RoamPage extends RoamBlock {
  title?: string
  'create-time'?: number
  'edit-time'?: number
}

/** A Roam block's text as markdown. */
export function roamText(
  text: string,
  blocks: Map<string, string>,
  title: string,
  report: Report,
): string {
  // references first: the block they bring may be a task itself
  return text
    .replace(
      /\{\{\[?\[?embed\]?\]?:\s*\(\(([\w-]{9})\)\)\}\}|\(\(([\w-]{9})\)\)/g,
      (all, a?: string, b?: string) => {
        const found = blocks.get(a ?? b ?? '')
        if (found === undefined) report.add({ kind: 'linkMissing', note: title, detail: all })
        return found ?? ''
      },
    )
    .replace(/\{\{\[\[TODO\]\]\}\}\s?|\{\{TODO\}\}\s?/g, '[ ] ')
    .replace(/\{\{\[\[DONE\]\]\}\}\s?|\{\{DONE\}\}\s?/g, '[x] ')
    .replace(/\{\{[^}]*\}\}/g, (all) => {
      report.add({ kind: 'dropped', note: title, detail: all })
      return ''
    })
    .replace(/(^|\s)#\[\[([^\]]+)\]\]/g, '$1[[$2]]')
    .replace(/__([^_\n]+)__/g, '*$1*')
    .replace(/\^\^([^^\n]+)\^\^/g, '$1')
}

function roamBlocks(
  children: RoamBlock[],
  blocks: Map<string, string>,
  title: string,
  report: Report,
  depth = 0,
): string {
  return children
    .map((block) => {
      const line = `${'  '.repeat(depth)}- ${roamText(block.string ?? '', blocks, title, report).replace(/\n/g, ' ')}`
      return block.children?.length
        ? `${line}\n${roamBlocks(block.children, blocks, title, report, depth + 1)}`
        : line
    })
    .join('\n')
}

export const roamImporter: Importer = {
  id: 'roam',
  app: 'Roam Research',
  picks: ['file'],
  extensions: ['json', 'zip'],
  detect: (source) => (source.entries.some((e) => extension(e.path) === 'json') ? 0.2 : 0),
  read: async (source, report) => {
    const notes: ModelNote[] = []
    for (const path of source.entries.map((e) => e.path).filter((p) => extension(p) === 'json')) {
      let pages: RoamPage[]
      try {
        pages = JSON.parse(await readText(source, path)) as RoamPage[]
        if (!Array.isArray(pages)) throw new Error('not pages')
      } catch {
        report.add({ kind: 'unreadable', detail: path })
        continue
      }
      const blocks = new Map<string, string>()
      const collect = (list: RoamBlock[] = []) => {
        for (const b of list) {
          if (b.uid) blocks.set(b.uid, b.string ?? '')
          collect(b.children)
        }
      }
      for (const page of pages) collect(page.children)
      for (const page of pages) {
        const title = page.title ?? ''
        const body = roamBlocks(page.children ?? [], blocks, title, report)
        const tags = inlineTags(body)
        for (const m of (page.children ? JSON.stringify(page.children) : '').matchAll(
          /#\[\[([^\]]+)\]\]/g,
        )) {
          tags.push(m[1] ?? '')
        }
        notes.push({
          key: page.uid ?? title,
          folder: null,
          title,
          body,
          tags,
          created: page['create-time'],
          updated: page['edit-time'],
          files: [],
        })
      }
    }
    return { folders: [], notes }
  },
}

/** A Tomboy note's content as markdown. */
function tomboyMarkdown(node: Node, titles: Set<string>): string {
  if (node.nodeType === 3) return node.textContent ?? ''
  if (!(node instanceof Element)) return ''
  const inner = () => [...node.childNodes].map((n) => tomboyMarkdown(n, titles)).join('')
  switch (node.localName) {
    case 'bold':
      return `**${inner()}**`
    case 'italic':
      return `*${inner()}*`
    case 'strikethrough':
      return `~~${inner()}~~`
    case 'monospace':
      return `\`${inner()}\``
    case 'internal': {
      const target = node.textContent ?? ''
      return titles.has(target.toLowerCase()) ? `[[${target}]]` : target
    }
    case 'url': {
      const url = node.textContent ?? ''
      return `[${url}](${/^\w+:/.test(url) ? url : `https://${url}`})`
    }
    case 'list':
      return `\n${[...node.children].map((item) => `- ${tomboyMarkdown(item, titles).trim()}`).join('\n')}\n`
    default:
      return inner()
  }
}

export const tomboyImporter: Importer = {
  id: 'tomboy',
  app: 'Tomboy, Gnote',
  picks: ['folder', 'file'],
  extensions: ['zip', 'note'],
  detect: (source) => (source.entries.some((e) => extension(e.path) === 'note') ? 0.9 : 0),
  read: async (source, report) => {
    const read: { path: string; xml: Document }[] = []
    for (const path of source.entries.map((e) => e.path).filter((p) => extension(p) === 'note')) {
      const xml = await readXml(source, path, report)
      if (xml) read.push({ path, xml })
    }
    const titleOf = (xml: Document) =>
      xml.getElementsByTagName('title')[0]?.textContent?.trim() ?? ''
    const titles = new Set(read.map(({ xml }) => titleOf(xml).toLowerCase()))
    const books = new Map<string, ModelFolder>()
    const notes: ModelNote[] = []
    for (const { path, xml } of read) {
      const tagsRaw = [...xml.getElementsByTagName('tag')].map((t) => t.textContent ?? '')
      // templates are the app's own
      if (tagsRaw.some((t) => t.includes('system:template'))) continue
      const book = tagsRaw
        .find((t) => t.startsWith('system:notebook:'))
        ?.slice('system:notebook:'.length)
      if (book && !books.has(book)) books.set(book, { key: book, parent: null, name: book })
      const content = xml.getElementsByTagName('note-content')[0]
      const title = titleOf(xml)
      const text = content ? tomboyMarkdown(content, titles) : ''
      // the content starts with the title again
      const body = text
        .replace(/^[^\n]*\n?/, '')
        .replace(/^\n+/, '')
        .trimEnd()
      notes.push({
        key: withoutExtension(baseName(path)),
        folder: book ?? null,
        title,
        body,
        tags: tagsRaw.filter((t) => !t.startsWith('system:')),
        created: time(xml.getElementsByTagName('create-date')[0]?.textContent),
        updated: time(xml.getElementsByTagName('last-change-date')[0]?.textContent),
        files: [],
      })
    }
    return { folders: [...books.values()], notes }
  },
}
