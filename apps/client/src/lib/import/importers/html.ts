// SPDX-License-Identifier: AGPL-3.0-or-later
// HTML exports: plain pages (Zoho Notebook, Nimbus, Trilium, saved pages)
// and Notion's HTML export. Links between pages become note links, pictures
// and files beside the pages become the notes' files, `data:` pictures are
// decoded, then the editor's schema writes the markdown.

import { dataUrl, htmlToMarkdown, parseHtml, taskList } from '../html'
import { NoteFiles } from '../markdown'
import { baseName, extension, readText, resolvePath, withoutExtension } from '../source'
import {
  fileRef,
  noteRef,
  type Importer,
  type ImportModel,
  type ImportSource,
  type ModelFolder,
  type ModelNote,
  type Report,
} from '../types'
import { folderOf, foldersOf, hidden } from './markdown'
import { notionFolders, notionId, withoutId } from './notion'

export const isHtml = (path: string) => ['html', 'htm'].includes(extension(path))

const isWeb = (target: string) => /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#')

const EXTENSIONS: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/svg+xml': 'svg',
}

/** What the HTML of a note refers to, made into notes and files. */
export interface DomLinks {
  from: string
  title: string
  /** The note an entry is, by its key, null when it is not a note. */
  noteKey: (path: string) => string | null
  /** Entries of the export. */
  entries: Set<string>
  files: NoteFiles
  report: Report
}

/**
 * Rewrites links, pictures and media of a page: to notes as wiki links, to
 * files of the export as the note's files, `data:` pictures decoded. Also
 * makes plain what the editor has no place for (details, figures).
 */
export function rewriteDom(root: Element, links: DomLinks) {
  const doc = root.ownerDocument
  const { from, title, files, report } = links
  const target = (url: string) => (isWeb(url) ? null : resolvePath(from, url))

  for (const a of [...root.querySelectorAll('a[href]')]) {
    const href = a.getAttribute('href') ?? ''
    const path = target(href)
    if (!path) continue
    const key = links.noteKey(path)
    if (key) {
      const wiki = doc.createElement('a')
      const fallback = withoutId(withoutExtension(baseName(path)))
      wiki.setAttribute('data-wiki', '')
      wiki.setAttribute('data-target', noteRef(key, fallback))
      const text = a.textContent?.trim() ?? ''
      if (text && text !== fallback) wiki.setAttribute('data-alias', text)
      a.replaceWith(wiki)
    } else if (links.entries.has(path)) {
      a.setAttribute('href', fileRef(files.add(path)))
    } else {
      // a link to nothing in the export is kept as its text
      report.add({ kind: isHtml(path) ? 'linkMissing' : 'fileMissing', note: title, detail: path })
      a.replaceWith(...a.childNodes)
    }
  }

  for (const img of [...root.querySelectorAll('img')]) {
    const src = img.getAttribute('src') ?? ''
    const data = dataUrl(src)
    if (data) {
      const ext = EXTENSIONS[data.mime] ?? 'bin'
      img.setAttribute('src', fileRef(files.addBytes(`image.${ext}`, data.mime, data.bytes)))
      continue
    }
    const path = target(src)
    if (!path) continue
    if (links.entries.has(path)) img.setAttribute('src', fileRef(files.add(path)))
    else {
      report.add({ kind: 'fileMissing', note: title, detail: path })
      img.remove()
    }
  }

  // media the editor does not play: a link to the file
  for (const media of [...root.querySelectorAll('video, audio, embed, object')]) {
    const src =
      media.getAttribute('src') ??
      media.getAttribute('data') ??
      media.querySelector('source')?.getAttribute('src') ??
      ''
    const path = target(src)
    if (path && links.entries.has(path)) {
      const name = files.add(path)
      const p = doc.createElement('p')
      const a = doc.createElement('a')
      a.setAttribute('href', fileRef(name))
      a.textContent = name
      p.append(a)
      media.replaceWith(p)
    }
  }

  for (const details of [...root.querySelectorAll('details')]) {
    const summary = details.querySelector('summary')
    const head = doc.createElement('p')
    const strong = doc.createElement('strong')
    strong.textContent = summary?.textContent ?? ''
    head.append(strong)
    summary?.remove()
    details.replaceWith(head, ...details.childNodes)
  }
  for (const figure of [...root.querySelectorAll('figure')]) {
    const caption = figure.querySelector('figcaption')
    if (caption) {
      const p = doc.createElement('p')
      p.append(...caption.childNodes)
      caption.replaceWith(p)
    }
    figure.replaceWith(...figure.childNodes)
  }
}

export interface HtmlNoteParts {
  title?: string
  tags?: string[]
  created?: number
  updated?: number
  pinned?: boolean
  trashed?: boolean
  key?: string
}

/** Reads pages of an export as notes. */
export async function readHtmlNotes(
  source: ImportSource,
  report: Report,
  options: {
    notes: string[]
    noteKey?: (path: string) => string
    folders?: ModelFolder[]
    folderOf?: (path: string) => string | null
    /** The element holding the note, the body by default. */
    content?: (doc: Document) => Element | null
    /** The app's own parts of the page, read and taken out first. */
    prepare?: (doc: Document, path: string) => HtmlNoteParts
  },
): Promise<ImportModel> {
  const noteSet = new Set(options.notes)
  const keyOf = options.noteKey ?? ((path: string) => path)
  const entries = new Set(source.entries.map((e) => e.path))
  const modified = new Map(source.entries.map((e) => [e.path, e.modified]))
  const folders = options.folders ?? foldersOf(options.notes, hidden)
  const kept = new Set(folders.map((f) => f.key))
  const notes: ModelNote[] = []
  let done = 0
  for (const path of options.notes) {
    report.progress(done++, options.notes.length)
    let doc: Document
    try {
      doc = parseHtml(await readText(source, path))
    } catch {
      report.add({ kind: 'unreadable', detail: path })
      continue
    }
    const parts = options.prepare?.(doc, path) ?? {}
    const root = options.content?.(doc) ?? doc.body
    const h1 = root.querySelector('h1')
    let title = (parts.title ?? doc.title ?? '').trim()
    if (!title && h1?.textContent?.trim()) title = h1.textContent.trim()
    // the title is kept apart, not twice
    if (h1 && h1.textContent?.trim() === title && root.firstElementChild === h1) h1.remove()
    title ||= withoutId(withoutExtension(baseName(path)))
    const files = new NoteFiles()
    rewriteDom(root, {
      from: path,
      title,
      noteKey: (p) => (noteSet.has(p) ? keyOf(p) : null),
      entries,
      files,
      report,
    })
    const { markdown, dropped } = htmlToMarkdown(root)
    for (const tag of dropped) report.add({ kind: 'dropped', note: title, detail: tag })
    const fileTime = modified.get(path) ?? undefined
    notes.push({
      key: parts.key ?? keyOf(path),
      folder: options.folderOf ? options.folderOf(path) : folderOf(path, kept),
      title,
      body: markdown,
      tags: parts.tags ?? [],
      created: parts.created ?? parts.updated ?? fileTime,
      updated: parts.updated ?? fileTime ?? parts.created,
      pinned: parts.pinned ?? false,
      trashed: parts.trashed ?? false,
      files: files.files,
    })
  }
  report.progress(options.notes.length, options.notes.length)
  return { folders, notes }
}

export const htmlImporter: Importer = {
  id: 'html',
  app: 'HTML',
  picks: ['folder', 'file'],
  extensions: ['zip', 'html', 'htm'],
  detect: (source) => (source.entries.some((e) => isHtml(e.path) && !hidden(e.path)) ? 0.25 : 0),
  read: (source, report) =>
    readHtmlNotes(source, report, {
      notes: source.entries.map((e) => e.path).filter((p) => isHtml(p) && !hidden(p)),
    }),
}

const notionTime = (text = '') => {
  const time = Date.parse(
    text
      .replace(/^@/, '')
      .replace(/\s*\([^)]*\)$/, '')
      .trim(),
  )
  return Number.isFinite(time) ? time : undefined
}

/** Notion's to-do lists as task lists. */
function notionTodos(doc: Document, root: Element) {
  for (const list of [...root.querySelectorAll('ul.to-do-list')]) {
    const items = [...list.children].map((li) => {
      const checked = !!li.querySelector('.checkbox-on')
      li.querySelector('.checkbox')?.remove()
      return { checked, content: [...li.childNodes] }
    })
    list.replaceWith(taskList(doc, items))
  }
}

/** Notion's HTML export, read by the Notion importer when it is HTML. */
export const notionHtmlImporter: Importer = {
  id: 'notion',
  app: 'Notion',
  picks: ['file', 'folder'],
  extensions: ['zip'],
  detect: (source) => {
    const html = source.entries.filter((e) => isHtml(e.path) && notionId(e.path)).length
    const md = source.entries.filter((e) => extension(e.path) === 'md' && notionId(e.path)).length
    return html > md ? 0.9 : 0
  },
  read: (source, report) => {
    const notes = source.entries.map((e) => e.path).filter((p) => isHtml(p) && !!notionId(p))
    const { folders, folderOf: notionFolderOf } = notionFolders(notes)
    return readHtmlNotes(source, report, {
      notes,
      folders,
      folderOf: notionFolderOf,
      noteKey: (path) => notionId(path) ?? path,
      content: (doc) => doc.querySelector('.page-body') ?? doc.querySelector('article'),
      prepare: (doc) => {
        const title = doc.querySelector('.page-title')?.textContent?.trim()
        const parts: HtmlNoteParts = { title, tags: [] }
        const table = doc.querySelector('table.properties')
        const lines: string[] = []
        for (const row of table ? [...table.querySelectorAll('tr')] : []) {
          const key = row.querySelector('th')?.textContent?.trim() ?? ''
          const cell = row.querySelector('td')
          // Notion writes dates as `@March 3, 2024`
          const value = cell?.textContent?.replace(/\s+/g, ' ').trim().replace(/^@/, '') ?? ''
          if (!key || !value) continue
          if (/^(created|created time)$/i.test(key)) parts.created = notionTime(value)
          if (/^(last edited time|updated)$/i.test(key)) parts.updated = notionTime(value)
          if (/^tags?$/i.test(key)) {
            const chips = [...(cell?.querySelectorAll('.selected-value') ?? [])]
            parts.tags = chips.length
              ? chips.map((c) => c.textContent?.trim() ?? '')
              : value.split(',')
          }
          lines.push(`${key}: ${value}`)
        }
        const body = doc.querySelector('.page-body') ?? doc.querySelector('article')
        if (body) {
          notionTodos(doc, body)
          if (lines.length) {
            const list = doc.createElement('ul')
            for (const line of lines) {
              const li = doc.createElement('li')
              li.textContent = line
              list.append(li)
            }
            body.prepend(list)
          }
        }
        return parts
      },
    })
  },
}
