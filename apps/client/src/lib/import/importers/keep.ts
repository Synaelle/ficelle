// SPDX-License-Identifier: AGPL-3.0-or-later
// Google Keep, from Google Takeout: `Takeout/Keep/` has a `.json` per note
// (text or a checklist, dates in microseconds, labels, pin, archive, trash,
// links found in the text) and its attachments beside it, whose extension
// can differ from the one the json names (`.jpeg` for `.jpg`).

import { htmlToMarkdown, parseHtml } from '../html'
import { NoteFiles } from '../markdown'
import { baseName, dirName, extension, readText, withoutExtension } from '../source'
import { fileRef, type Importer, type ModelNote } from '../types'

const isKeepNote = (path: string) =>
  /(^|\/)Keep\/[^/]+\.json$/i.test(path) && !/Labels\.json$/i.test(path)

interface KeepNote {
  title?: string
  textContent?: string
  textContentHtml?: string
  listContent?: { text?: string; isChecked?: boolean }[]
  createdTimestampUsec?: number
  userEditedTimestampUsec?: number
  isPinned?: boolean
  isArchived?: boolean
  isTrashed?: boolean
  labels?: { name?: string }[]
  attachments?: { filePath?: string; mimetype?: string }[]
  annotations?: { url?: string; title?: string; source?: string }[]
}

const micro = (value?: number) => (value ? Math.round(value / 1000) : undefined)

export const keepImporter: Importer = {
  id: 'keep',
  app: 'Google Keep',
  picks: ['file', 'folder'],
  extensions: ['zip', 'tgz'],
  detect: (source) => (source.entries.some((e) => isKeepNote(e.path)) ? 0.95 : 0),
  read: async (source, report) => {
    const paths = source.entries.map((e) => e.path)
    const notesPaths = paths.filter(isKeepNote)
    const notes: ModelNote[] = []
    let done = 0
    for (const path of notesPaths) {
      report.progress(done++, notesPaths.length)
      let keep: KeepNote
      try {
        keep = JSON.parse(await readText(source, path)) as KeepNote
      } catch {
        report.add({ kind: 'unreadable', detail: path })
        continue
      }
      const title = keep.title?.trim() ?? ''
      const files = new NoteFiles()
      const parts: string[] = []
      if (keep.listContent?.length) {
        parts.push(
          keep.listContent
            .map(
              (item) =>
                `- [${item.isChecked ? 'x' : ' '}] ${(item.text ?? '').replace(/\n/g, ' ')}`,
            )
            .join('\n'),
        )
      } else if (keep.textContentHtml) {
        const { markdown } = htmlToMarkdown(parseHtml(keep.textContentHtml).body)
        parts.push(markdown)
      } else if (keep.textContent) {
        // plain text: line breaks are kept as they were typed
        parts.push(keep.textContent.replace(/\n/g, '  \n'))
      }
      for (const attachment of keep.attachments ?? []) {
        const wanted = withoutExtension(baseName(attachment.filePath ?? ''))
        const dir = dirName(path)
        const entry =
          paths.find((p) => p === `${dir}/${attachment.filePath}`) ??
          paths.find(
            (p) =>
              dirName(p) === dir &&
              withoutExtension(baseName(p)) === wanted &&
              extension(p) !== 'json',
          )
        if (!entry) {
          report.add({ kind: 'fileMissing', note: title, detail: attachment.filePath })
          continue
        }
        const name = files.add(entry)
        parts.push(
          (attachment.mimetype ?? '').startsWith('image/') ||
            /^(png|jpe?g|gif|webp)$/.test(extension(entry))
            ? `![](${fileRef(name)})`
            : `[${name}](${fileRef(name)})`,
        )
      }
      const links = (keep.annotations ?? []).filter((a) => a.url)
      if (links.length) parts.push(links.map((a) => `- [${a.title || a.url}](${a.url})`).join('\n'))
      const tags = (keep.labels ?? []).map((l) => l.name ?? '').filter(Boolean)
      if (keep.isArchived) tags.push('archived')
      notes.push({
        key: baseName(path),
        folder: null,
        title,
        body: parts.join('\n\n'),
        tags,
        created: micro(keep.createdTimestampUsec),
        updated: micro(keep.userEditedTimestampUsec),
        pinned: keep.isPinned === true,
        trashed: keep.isTrashed === true,
        files: files.files,
      })
    }
    report.progress(notesPaths.length, notesPaths.length)
    return { folders: [], notes }
  },
}
