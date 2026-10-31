// SPDX-License-Identifier: AGPL-3.0-or-later
// Joplin: a JEX file (a tar) or the RAW export folder. Every item is a
// `<id>.md`: title, blank line, body, then `key: value` lines ending with
// `type_` (1 note, 2 folder, 4 resource, 5 tag, 6 a note's tag). Files are
// in `resources/`, and bodies point at files and notes alike as `:/<id>`.

import { htmlToMarkdown, parseHtml } from '../html'
import { NoteFiles } from '../markdown'
import { baseName, readText, withoutExtension } from '../source'
import {
  fileRef,
  noteRef,
  type Importer,
  type ModelFolder,
  type ModelNote,
  type Report,
} from '../types'

const ITEM = /^(?:.*\/)?[0-9a-f]{32}\.md$/
const isItem = (path: string) => ITEM.test(path) && !path.includes('resources/')

export interface JoplinItem {
  title: string
  body: string
  meta: Record<string, string>
}

/** One serialized item: the properties are the `key: value` lines at the end. */
export function parseItem(text: string): JoplinItem {
  const lines = text.replace(/\r\n?/g, '\n').split('\n')
  const meta: Record<string, string> = {}
  while (lines.length) {
    const prop = /^([a-z_]+): ?(.*)$/.exec(lines[lines.length - 1] ?? '')
    if (!prop) break
    meta[prop[1] ?? ''] = (prop[2] ?? '').replace(/\\n/g, '\n')
    lines.pop()
  }
  while (lines.length && !lines[lines.length - 1]?.trim()) lines.pop()
  const title = lines.shift() ?? ''
  if (lines[0] === '') lines.shift()
  return { title, body: lines.join('\n'), meta }
}

const time = (value?: string) => {
  if (!value || value === '0') return undefined
  const parsed = /^\d+$/.test(value) ? Number(value) : Date.parse(value)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined
}

const ID_LINK = ':/([0-9a-f]{32})'

/** What `:/<id>` means: a file of the export or another note. */
interface Targets {
  resources: Map<string, { entry: string; name: string; mime: string }>
  notes: Map<string, string>
}

function rewriteMarkdown(
  body: string,
  title: string,
  targets: Targets,
  files: NoteFiles,
  report: Report,
) {
  return body.replace(
    new RegExp(`(!?)\\[([^\\]\\n]*)\\]\\(${ID_LINK}(#[^)\\s]*)?\\)`, 'g'),
    (all, bang: string, label: string, id: string, anchor = '') => {
      const resource = targets.resources.get(id)
      if (resource) {
        const name = files.add(resource.entry, resource.name)
        return `${bang}[${label}](${fileRef(name)})`
      }
      const note = targets.notes.get(id)
      if (note !== undefined) {
        const heading = anchor ? `#${decodeURIComponent(anchor.slice(1)).replace(/-/g, ' ')}` : ''
        return `[[${noteRef(id, note)}${heading}${label && label !== note ? `|${label.replace(/[|\]]/g, '')}` : ''}]]`
      }
      report.add({ kind: bang ? 'fileMissing' : 'linkMissing', note: title, detail: label || id })
      return label
    },
  )
}

function rewriteHtml(
  body: string,
  title: string,
  targets: Targets,
  files: NoteFiles,
  report: Report,
) {
  const doc = parseHtml(body)
  const id = (url: string | null) => new RegExp(`^${ID_LINK}`).exec(url ?? '')?.[1]
  for (const el of [...doc.body.querySelectorAll('img[src], a[href]')]) {
    const attribute = el.localName === 'img' ? 'src' : 'href'
    const target = id(el.getAttribute(attribute))
    if (!target) continue
    const resource = targets.resources.get(target)
    const note = targets.notes.get(target)
    if (resource) el.setAttribute(attribute, fileRef(files.add(resource.entry, resource.name)))
    else if (note !== undefined && el.localName === 'a') {
      const wiki = doc.createElement('a')
      wiki.setAttribute('data-wiki', '')
      wiki.setAttribute('data-target', noteRef(target, note))
      el.replaceWith(wiki)
    } else {
      report.add({ kind: 'fileMissing', note: title, detail: target })
      el.remove()
    }
  }
  const { markdown, dropped } = htmlToMarkdown(doc.body)
  for (const tag of dropped) report.add({ kind: 'dropped', note: title, detail: tag })
  return markdown
}

export const joplinImporter: Importer = {
  id: 'joplin',
  app: 'Joplin',
  picks: ['file', 'folder'],
  extensions: ['jex', 'tar'],
  detect: (source) => {
    if (/\.jex$/i.test(source.name)) return 0.95
    const items = source.entries.filter((e) => isItem(e.path)).length
    const markdown = source.entries.filter((e) => /\.md$/i.test(e.path)).length
    return items > 0 && items / markdown > 0.5 ? 0.9 : 0
  },
  read: async (source, report) => {
    const paths = source.entries.map((e) => e.path)
    const items = new Map<string, JoplinItem>()
    let done = 0
    for (const path of paths.filter(isItem)) {
      report.progress(done++, paths.length)
      try {
        items.set(withoutExtension(baseName(path)), parseItem(await readText(source, path)))
      } catch {
        report.add({ kind: 'unreadable', detail: path })
      }
    }
    const of = (type: string) => [...items].filter(([, item]) => item.meta.type_ === type)
    const resourceEntry = (id: string) =>
      paths.find((p) => new RegExp(`(^|/)resources/${id}(\\.|$)`).test(p))
    const targets: Targets = { resources: new Map(), notes: new Map() }
    for (const [id, item] of of('4')) {
      const entry = resourceEntry(id)
      const ext = item.meta.file_extension ? `.${item.meta.file_extension}` : ''
      const name =
        item.meta.filename || (item.title.includes('.') ? item.title : `${item.title || id}${ext}`)
      if (entry) targets.resources.set(id, { entry, name, mime: item.meta.mime ?? '' })
    }
    for (const [id, item] of of('1')) targets.notes.set(id, item.title)

    const tagNames = new Map(of('5').map(([id, item]) => [id, item.title]))
    const tagsOf = new Map<string, string[]>()
    for (const [, link] of of('6')) {
      const name = tagNames.get(link.meta.tag_id ?? '')
      const note = link.meta.note_id ?? ''
      if (name) tagsOf.set(note, [...(tagsOf.get(note) ?? []), name])
    }

    const folderIds = new Set(of('2').map(([id]) => id))
    const folders: ModelFolder[] = of('2').map(([id, item]) => ({
      key: id,
      parent: folderIds.has(item.meta.parent_id ?? '') ? (item.meta.parent_id ?? null) : null,
      name: item.title,
    }))
    const notes: ModelNote[] = []
    for (const [id, item] of of('1')) {
      const { meta } = item
      if (meta.encryption_applied === '1') {
        report.add({ kind: 'encrypted', detail: id })
        continue
      }
      // a conflict copy is Joplin's own, the note itself comes in
      if (meta.is_conflict === '1') continue
      const files = new NoteFiles()
      const body =
        meta.markup_language === '2'
          ? rewriteHtml(item.body, item.title, targets, files, report)
          : rewriteMarkdown(item.body, item.title, targets, files, report)
      const tags = [...(tagsOf.get(id) ?? [])]
      if (meta.is_todo === '1') tags.push(time(meta.todo_completed) ? 'done' : 'todo')
      notes.push({
        key: id,
        folder: folderIds.has(meta.parent_id ?? '') ? (meta.parent_id ?? null) : null,
        title: item.title,
        body,
        tags,
        created: time(meta.user_created_time) ?? time(meta.created_time),
        updated: time(meta.user_updated_time) ?? time(meta.updated_time),
        trashed: !!time(meta.deleted_time),
        files: files.files,
      })
    }
    report.progress(paths.length, paths.length)
    return { folders, notes }
  },
}
