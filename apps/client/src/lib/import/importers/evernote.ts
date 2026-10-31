// SPDX-License-Identifier: AGPL-3.0-or-later
// Evernote: one `.enex` file per notebook (XML). Each note has its title,
// dates, tags, its text in ENML (HTML with Evernote's own elements) and its
// files in base64, which the text names by the MD5 of their bytes. Links
// between notes carry no id ENEX keeps: they are found by title.

import { base64Bytes, htmlToMarkdown, parseHtml, taskList } from '../html'
import { parseDate, NoteFiles } from '../markdown'
import { md5 } from '../md5'
import { baseName, extension, readText, withoutExtension } from '../source'
import { fileRef, noteRef, type Importer, type ModelFolder, type ModelNote } from '../types'

const isEnex = (path: string) => extension(path) === 'enex'

const childText = (el: Element, name: string) =>
  [...el.children].find((c) => c.localName === name)?.textContent?.trim() ?? ''

/** Evernote's links to its notes, by title since ENEX keeps no ids. */
const NOTE_LINK = /^(evernote:\/\/\/view\/|https?:\/\/(www\.)?evernote\.com\/(shard|l)\/)/i

/**
 * CDATA sections as escaped text: the same once parsed, and read alike by
 * every XML parser (the tests' one has no CDATA).
 */
const withoutCdata = (xml: string) =>
  xml.replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, (_all, text: string) =>
    text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'),
  )

/** Self-closing Evernote elements, which the HTML parser would not close. */
const closeEmpty = (enml: string) =>
  enml.replace(/<(en-media|en-todo|en-crypt)\b([^>]*?)\/>/g, '<$1$2></$1>')

/** Evernote's checkboxes, each in its own line, as task lists. */
function todos(doc: Document, root: Element) {
  for (const todo of [...root.querySelectorAll('en-todo')]) {
    const block = todo.closest('div, p, li') ?? todo.parentElement
    const checked = todo.getAttribute('checked') === 'true'
    if (!block || block === root) {
      todo.replaceWith(doc.createTextNode(checked ? '[x] ' : '[ ] '))
      continue
    }
    todo.remove()
    block.replaceWith(taskList(doc, [{ checked, content: [...block.childNodes] }]))
  }
  // checkboxes on following lines make one list
  for (const list of [...root.querySelectorAll('ul[data-type="taskList"]')]) {
    const previous = list.previousElementSibling
    if (previous?.matches('ul[data-type="taskList"]')) {
      previous.append(...list.children)
      list.remove()
    }
  }
}

export const evernoteImporter: Importer = {
  id: 'evernote',
  app: 'Evernote',
  picks: ['file', 'folder'],
  extensions: ['enex', 'zip'],
  detect: (source) => (source.entries.some((e) => isEnex(e.path)) ? 0.95 : 0),
  read: async (source, report) => {
    const books = source.entries.map((e) => e.path).filter(isEnex)
    const folders: ModelFolder[] =
      books.length > 1
        ? books.map((path) => ({ key: path, parent: null, name: withoutExtension(baseName(path)) }))
        : []
    // read every note first: links between notes are found by title
    const read: { book: string; note: Element; key: string; title: string }[] = []
    let done = 0
    for (const book of books) {
      report.progress(done++, books.length)
      let xml: Document
      try {
        xml = new DOMParser().parseFromString(
          withoutCdata(await readText(source, book)),
          'application/xml',
        )
      } catch {
        report.add({ kind: 'unreadable', detail: book })
        continue
      }
      if (xml.querySelector('parsererror')) {
        report.add({ kind: 'unreadable', detail: book })
        continue
      }
      for (const note of [...xml.getElementsByTagName('note')]) {
        const title = childText(note, 'title')
        read.push({
          book,
          note,
          title,
          key: `${baseName(book)}\n${title}\n${childText(note, 'created')}`,
        })
      }
    }
    const byTitle = new Map<string, string>()
    for (const r of read)
      if (!byTitle.has(r.title.toLowerCase())) byTitle.set(r.title.toLowerCase(), r.key)

    const notes: ModelNote[] = []
    for (const { book, note, title, key } of read) {
      const files = new NoteFiles()
      // the note's files by the MD5 of their bytes
      const resources = new Map<string, { name: string; mime: string; bytes: Uint8Array }>()
      for (const resource of [...note.getElementsByTagName('resource')]) {
        const data = [...resource.children].find((c) => c.localName === 'data')?.textContent ?? ''
        const mime = childText(resource, 'mime') || 'application/octet-stream'
        const attributes = [...resource.children].find((c) => c.localName === 'resource-attributes')
        const ext = mime.split('/')[1]?.replace('jpeg', 'jpg').replace(/\W.*$/, '') ?? 'bin'
        const name = (attributes && childText(attributes, 'file-name')) || `file.${ext}`
        try {
          const bytes = base64Bytes(data)
          resources.set(md5(bytes), { name, mime, bytes })
        } catch {
          report.add({ kind: 'unreadable', note: title, detail: name })
        }
      }

      const doc = parseHtml(closeEmpty(childText(note, 'content')))
      const root = doc.querySelector('en-note') ?? doc.body
      todos(doc, root)
      for (const crypt of [...root.querySelectorAll('en-crypt')]) {
        report.add({ kind: 'encrypted', note: title })
        crypt.remove()
      }
      for (const media of [...root.querySelectorAll('en-media')]) {
        const found = resources.get(media.getAttribute('hash')?.toLowerCase() ?? '')
        if (!found) {
          report.add({ kind: 'fileMissing', note: title, detail: media.getAttribute('type') ?? '' })
          media.remove()
          continue
        }
        const name = files.addBytes(found.name, found.mime, found.bytes)
        let element: HTMLElement
        if (found.mime.startsWith('image/')) {
          element = doc.createElement('img')
          element.setAttribute('src', fileRef(name))
          element.setAttribute('alt', withoutExtension(name))
        } else {
          element = doc.createElement('a')
          element.setAttribute('href', fileRef(name))
          element.textContent = name
        }
        media.replaceWith(element)
      }
      for (const a of [...root.querySelectorAll('a[href]')]) {
        if (!NOTE_LINK.test(a.getAttribute('href') ?? '')) continue
        const text = a.textContent?.trim() ?? ''
        const target = byTitle.get(text.toLowerCase())
        const wiki = doc.createElement('a')
        wiki.setAttribute('data-wiki', '')
        wiki.setAttribute('data-target', noteRef(target ?? `missing:${text}`, text))
        a.replaceWith(wiki)
      }
      const { markdown, dropped } = htmlToMarkdown(root)
      for (const tag of dropped) report.add({ kind: 'dropped', note: title, detail: tag })
      const attributes = [...note.children].find((c) => c.localName === 'note-attributes')
      const url = attributes ? childText(attributes, 'source-url') : ''
      notes.push({
        key,
        folder: folders.length ? book : null,
        title,
        body: url && !markdown.includes(url) ? `${markdown}\n\n<${url}>` : markdown,
        tags: [...note.getElementsByTagName('tag')].map((t) => t.textContent?.trim() ?? ''),
        created: parseDate(childText(note, 'created')),
        updated: parseDate(childText(note, 'updated')),
        files: files.files,
      })
    }
    report.progress(books.length, books.length)
    return { folders, notes }
  },
}
